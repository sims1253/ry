import { errorMessage } from "./errors";
import * as vscode from "vscode";
import { type OutputChannel } from "vscode";
import { type LanguageClientOptions, State } from "vscode-languageclient";
import {
  LanguageClient,
  RevealOutputChannelOn,
} from "vscode-languageclient/node";
import { LOG_CHANNEL_NAME, RY_SERVER_SUBCOMMAND } from "./constants";
import { logger } from "./logger";
import {
  getExtensionSettings,
  getGlobalSettings,
  getWorkspaceSettings,
} from "./settings";

/**
 * Construct and start the language server client.
 *
 * `binaryPath` must be pre-resolved by the caller via
 * `findRyBinaryPath()` (which honors workspace trust) so the launched
 * binary is the same one that was version-gated and displayed.
 */
export async function startServer(
  namespace: string,
  binaryPath: string,
  outputChannel: OutputChannel,
  traceOutputChannel: OutputChannel,
) {
  // The server falls back to `globalSettings` when `settings` is empty.
  const initializationOptions = {
    settings: getExtensionSettings(namespace),
    globalSettings: getGlobalSettings(namespace),
  };
  logger.info(
    `Initialization options: ${JSON.stringify(initializationOptions, null, 4)}`,
  );

  const logLevel = vscode.workspace
    .getConfiguration(namespace)
    .get<string>("logLevel");
  const serverArgs: string[] = logLevel
    ? [RY_SERVER_SUBCOMMAND, "--log-level", logLevel]
    : [RY_SERVER_SUBCOMMAND];
  logger.info(
    `ry language server command: '${[binaryPath, ...serverArgs].join(" ")}'`,
  );

  const serverOptions = { command: binaryPath, args: serverArgs };

  const clientOptions: LanguageClientOptions = {
    // Register the server for R documents (and ry.toml).
    documentSelector: [
      { scheme: "file", language: "r" },
      { scheme: "untitled", language: "r" },
      { scheme: "vscode-notebook", language: "r" },
      { scheme: "vscode-notebook-cell", language: "r" },
      { scheme: "file", pattern: "**/{ry.toml}" },
    ],
    outputChannel,
    traceOutputChannel,
    revealOutputChannelOn: RevealOutputChannelOn.Never,
    initializationOptions,
    synchronize: { configurationSection: namespace },
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

  newLSClient.onDidChangeState((e) =>
    logger.debug(`Server State: ${State[e.newState]}`),
  );

  logger.info("Server: Start requested.");
  try {
    await newLSClient.start();
  } catch (error) {
    try {
      await newLSClient.dispose();
    } catch (cleanupError) {
      logger.error(`Server cleanup failed: ${errorMessage(cleanupError)}`);
    }
    throw new Error(
      `Server failed to start at ${binaryPath}: ${errorMessage(error)}`,
    );
  }
  return newLSClient;
}

export function stopServer(client: LanguageClient): Promise<void> {
  logger.info("Server: Stop requested");
  return client.dispose();
}
