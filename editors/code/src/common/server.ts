import * as fs from "fs/promises";
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
import {
  BUNDLED_RY_EXECUTABLE,
  LOG_CHANNEL_NAME,
  RY_BINARY_NAME,
  RY_SERVER_SUBCOMMAND,
} from "./constants";
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

/**
 * Resolve the path to the `ry` binary.
 *
 * Minimal E1 strategy:
 *   1. the first existing entry in the `ry.path` setting;
 *   2. the bundled binary, if present;
 *   3. the bare command name, letting the OS resolve it via `PATH`.
 *
 * E2 replaces this with version gating, workspace-trust handling, and
 * import-strategy logic.
 */
async function resolveBinary(namespace: string): Promise<string> {
  const config = getConfiguration(namespace);
  const candidatePaths = config.get<string[]>("path") ?? [];
  for (const candidate of candidatePaths) {
    try {
      await fs.access(candidate);
      logger.info(`Using 'path' setting: ${candidate}`);
      return candidate;
    } catch {
      // Not found; try the next candidate.
    }
  }

  try {
    await fs.access(BUNDLED_RY_EXECUTABLE);
    logger.info(`Using bundled executable: ${BUNDLED_RY_EXECUTABLE}`);
    return BUNDLED_RY_EXECUTABLE;
  } catch {
    // No bundled binary; fall through to the environment.
  }

  logger.info(`Using environment executable: ${RY_BINARY_NAME}`);
  return RY_BINARY_NAME;
}

let _disposables: Disposable[] = [];

export type ServerState = {
  client: LanguageClient;
};



/**
 * Construct and start the language server client.
 */
export async function startServer(
  namespace: string,
  outputChannel: OutputChannel,
  traceOutputChannel: OutputChannel,
): Promise<ServerState | null> {
  const binaryPath = await resolveBinary(namespace);

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
              ? vscode.workspace.getWorkspaceFolder(vscode.Uri.parse(item.scopeUri))
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