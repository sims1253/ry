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

/**
 * Per-folder settings, mirroring the server's `FolderSettings`
 * (`crates/ry-lsp/src/settings.rs`).
 *
 * Every field is optional so that "unset" (fall back to `ry.toml` or the
 * built-in default) is distinguishable from "set to the default value".
 * This is the precedence rule from the plan: editor setting > `ry.toml`
 * > built-in default.
 *
 * Field names are camelCase to match the server's
 * `#[serde(rename_all = "camelCase")]`.
 */
export type FolderSettings = {
  configuration?: string;
  lint?: {
    select?: string[];
    extendSelect?: string[];
    ignore?: string[];
    error?: string[];
    warn?: string[];
  };
  minConfidence?: string;
  baseline?: string;
  checkTestFixtures?: boolean;
  logLevel?: string;
  enable?: boolean;
  path?: string[];
  importStrategy?: string;
  addExecutableToTerminalPath?: boolean;
};

/**
 * The initialization options envelope sent to the server, matching
 * ruff-vscode's shape and the server's `ServerSettings`: an array of
 * per-workspace-folder settings plus a global fallback, all up-front at
 * `initialize`.
 */
export type InitializationOptions = {
  settings: FolderSettings[];
  globalSettings: FolderSettings;
};

/**
 * Read a single folder's settings from the VS Code configuration.
 *
 * This is the E1 placeholder for E3's `settings.ts` `getWorkspaceSettings`:
 * it reads the global `ry.*` configuration and maps it into the
 * `FolderSettings` shape the server expects. Fields absent from
 * `contributes.configuration` are read as `undefined` and omitted, so
 * "unset" reaches the server untouched.
 */
function readFolderSettings(namespace: string): FolderSettings {
  const config = getConfiguration(namespace);
  return {
    configuration: config.get<string>("configuration"),
    lint: {
      select: config.get<string[]>("lint.select"),
      extendSelect: config.get<string[]>("lint.extendSelect"),
      ignore: config.get<string[]>("lint.ignore"),
      error: config.get<string[]>("lint.error"),
      warn: config.get<string[]>("lint.warn"),
    },
    minConfidence: config.get<string>("minConfidence"),
    baseline: config.get<string>("baseline"),
    checkTestFixtures: config.get<boolean>("checkTestFixtures"),
    logLevel: config.get<string>("logLevel"),
    enable: config.get<boolean>("enable"),
    path: config.get<string[]>("path"),
    importStrategy: config.get<string>("importStrategy"),
    addExecutableToTerminalPath: config.get<boolean>(
      "addExecutableToTerminalPath",
    ),
  };
}

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
  const globalSettings = readFolderSettings(namespace);
  return {
    settings: [globalSettings],
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

export type ServerState = {
  client: LanguageClient;
};

let _disposables: Disposable[] = [];

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

  const serverArgs: string[] = [RY_SERVER_SUBCOMMAND];
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
    dispose();
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
  dispose();
}

function dispose(): void {
  for (const disposable of _disposables) {
    disposable.dispose();
  }
  _disposables = [];
}
