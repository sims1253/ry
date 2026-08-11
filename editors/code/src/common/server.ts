import * as vscode from "vscode";
import { type Disposable, type OutputChannel } from "vscode";
import {
  type LanguageClientOptions,
  MessageType,
  ShowMessageNotification,
  State,
} from "vscode-languageclient";
import {
  LanguageClient,
  RevealOutputChannelOn,
} from "vscode-languageclient/node";
import { LOG_CHANNEL_NAME, RY_SERVER_SUBCOMMAND } from "./constants";
import { logger } from "./logger";
import { getDocumentSelector } from "./utilities";
import { getConfiguration } from "./vscodeapi";
import {
  type ISettings,
  getExtensionSettings,
  getGlobalSettings,
  getWorkspaceSettings,
} from "./settings";

/**
 * The initialization options envelope sent to the server.
 */
export type InitializationOptions = {
  settings: ISettings[];
  globalSettings: ISettings;
};

/**
 * Build the `initializationOptions` to send at `initialize`.
 *
 * Returns the global settings as both the single folder entry and the
 * global fallback. E3's `getExtensionSettings` will replace this with a
 * proper per-workspace-folder array (consumed by S4 multi-root support).
 */
export function getInitializationOptions(
  namespace: string,
): InitializationOptions {
  const folderSettings = getExtensionSettings(namespace);
  const globalSettings = getGlobalSettings(namespace);
  return {
    settings: folderSettings.length > 0 ? folderSettings : [globalSettings],
    globalSettings,
  };
}

let _disposables: Disposable[] = [];

export type ServerState = {
  client: LanguageClient;
};

/**
 * Construct and start the language server client.
 *
 * P37-W2: `binaryPath` is the already-resolved binary path from
 * `extension.ts`, which called `findRyBinaryPath()` with the correct
 * `isUntrusted` flag. The server no longer resolves its own binary —
 * eliminating the split-brain where a different binary could be
 * version-gated/displayed vs. launched.
 */
export async function startServer(
  namespace: string,
  binaryPath: string,
  outputChannel: OutputChannel,
  traceOutputChannel: OutputChannel,
): Promise<ServerState | null> {
  const initializationOptions = getInitializationOptions(namespace);
  logger.info(
    `Initialization options: ${JSON.stringify(initializationOptions, null, 4)}`,
  );

  // M10: Pass --log-level to the server if configured.
  const logLevel = getConfiguration(namespace).get<string>("logLevel");
  const serverArgs: string[] = logLevel
    ? [RY_SERVER_SUBCOMMAND, "--log-level", logLevel]
    : [RY_SERVER_SUBCOMMAND];
  logger.info(
    `ry language server command: '${[binaryPath, ...serverArgs].join(" ")}'`,
  );

  const serverOptions = {
    command: binaryPath,
    args: serverArgs,
    options: { env: process.env },
  };

  const clientOptions: LanguageClientOptions = {
    // Register the server for R documents (and ry.toml).
    documentSelector: getDocumentSelector(),
    outputChannel,
    traceOutputChannel,
    revealOutputChannelOn: RevealOutputChannelOn.Never,
    initializationOptions,
    middleware: {
      workspace: {
        configuration: async (params, token, next) => {
          const values = await next(params, token);
          if (!Array.isArray(values)) return values;
          return params.items.map((item, index) => {
            if (item.section !== namespace) return values[index];
            const folder = item.scopeUri
              ? vscode.workspace.getWorkspaceFolder(
                  vscode.Uri.parse(item.scopeUri),
                )
              : undefined;
            return folder
              ? getWorkspaceSettings(namespace, folder)
              : getGlobalSettings(namespace);
          });
        },
      },
    },
  };

  const newLSClient = new LanguageClient(
    namespace,
    `${LOG_CHANNEL_NAME} Language Server`,
    serverOptions,
    clientOptions,
  );

  _disposables.push(
    newLSClient.onDidChangeState((e) => {
      switch (e.newState) {
        case State.Stopped:
          logger.debug("Server State: Stopped");
          break;
        case State.Starting:
          logger.debug("Server State: Starting");
          break;
        case State.Running:
          logger.debug("Server State: Running");
          break;
      }
    }),
    // Intercept `window/showMessage` from the server and attach a
    // "Show Logs" button, turning an opaque server error into something
    // actionable.
    newLSClient.onNotification(ShowMessageNotification.type, (params) => {
      const showMessageMethod =
        params.type === MessageType.Error
          ? vscode.window.showErrorMessage
          : params.type === MessageType.Warning
            ? vscode.window.showWarningMessage
            : vscode.window.showInformationMessage;
      showMessageMethod(params.message, "Show Logs").then((selection) => {
        if (selection) {
          outputChannel.show();
        }
      });
    }),
  );

  logger.info("Server: Start requested.");
  try {
    await newLSClient.start();
  } catch (ex) {
    logger.error(`Server: Start failed: ${ex}`);
    dispose(newLSClient);
    return null;
  }

  return { client: newLSClient };
}

/**
 * Stop the language server client.
 */
export async function stopServer(lsClient: LanguageClient): Promise<void> {
  logger.info("Server: Stop requested");
  await lsClient.stop();
  dispose(lsClient);
}

function dispose(client?: LanguageClient): void {
  for (const disposable of _disposables) {
    disposable.dispose();
  }
  _disposables = [];
  client?.dispose();
}
