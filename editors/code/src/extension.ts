import * as vscode from "vscode";
import { LOG_CHANNEL_NAME, RY_SETTINGS_NAMESPACE } from "./common/constants";
import { LazyOutputChannel, logger } from "./common/logger";
import { type ServerState, startServer, stopServer } from "./common/server";
import {
  getConfiguration,
  onDidChangeConfiguration,
  registerCommand,
} from "./common/vscodeapi";
import {
  getWorkspaceSettings,
  checkIfConfigurationChanged,
} from "./common/settings";
import {
  findRyBinaryPath,
  getRyVersion,
  checkVersionCapability,
  type ResolvedBinary,
  MINIMUM_VERSION,
} from "./common/binary";
import { StatusItem } from "./common/status";
import { debugInformationCommand, explainRuleCommand } from "./common/commands";

let serverState: ServerState | null = null;
let restartQueued = false;
let restartPromise: Promise<void> | null = null;
let statusItem: StatusItem | null = null;
let resolvedBinary: ResolvedBinary | null = null;

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  const serverId = RY_SETTINGS_NAMESPACE;

  logger.info(`Name: ${LOG_CHANNEL_NAME}`);
  logger.info(`Module: ${serverId}`);

  const outputChannel = vscode.window.createOutputChannel(
    `${LOG_CHANNEL_NAME} Language Server`,
  );
  const traceOutputChannel = new LazyOutputChannel(
    `${LOG_CHANNEL_NAME} Language Server Trace`,
  );

  context.subscriptions.push(outputChannel);
  context.subscriptions.push(traceOutputChannel);
  context.subscriptions.push(logger.channel);

  // Status item shows the resolved binary path and version.
  statusItem = new StatusItem("ry-status");
  statusItem.setBusy();
  context.subscriptions.push(statusItem);

  // The `ry.enable` gate: return early from activation when disabled.
  const enable = getConfiguration(serverId).get<boolean>("enable", true);
  if (!enable) {
    logger.info(
      `Extension is disabled. To enable, change \`${serverId}.enable\` to \`true\` and restart VS Code.`,
    );
    statusItem.setWarning("Extension disabled");
    return;
  }

  // E2: Resolve the binary and probe its version before starting.
  const isUntrusted = !vscode.workspace.isTrusted;
  const settings = getWorkspaceSettings(serverId, {
    uri: vscode.Uri.file(process.cwd()),
    index: 0,
    name: "root",
  } as vscode.WorkspaceFolder);
  const binaryPath = findRyBinaryPath(settings, isUntrusted);
  const version = getRyVersion(binaryPath);
  resolvedBinary = { path: binaryPath, version };

  if (version) {
    const versionError = checkVersionCapability(
      resolvedBinary,
      MINIMUM_VERSION,
      "settings channel",
    );
    if (versionError) {
      statusItem.setError(versionError);
      vscode.window.showErrorMessage(versionError);
      return;
    }
  }

  statusItem.setReady(resolvedBinary);

  const runServer = async () => {
    if (serverState != null) {
      await stopServer(serverState.client);
      serverState = null;
    }

    statusItem?.setBusy();
    serverState = await startServer(
      serverId,
      resolvedBinary!.path,
      outputChannel,
      traceOutputChannel,
    );
    if (serverState) {
      if (resolvedBinary) statusItem?.setReady(resolvedBinary);
    } else {
      statusItem?.setError("Server failed to start");
    }
  };

  // Restart orchestration: at most one restart runs, at most one pends.
  const requestRestart = async () => {
    if (restartPromise != null) {
      if (!restartQueued) {
        logger.info(
          `${LOG_CHANNEL_NAME} restart requested while another restart is in progress; queuing one more restart.`,
        );
        restartQueued = true;
      }
      await restartPromise;
      return;
    }

    restartQueued = false;
    restartPromise = (async () => {
      try {
        do {
          restartQueued = false;
          await runServer();
        } while (restartQueued);
      } finally {
        restartPromise = null;
      }
    })();
    await restartPromise;
  };

  // E3: Configuration change triggers restart only for settings
  // that need a respawn. Live-updatable settings go via
  // didChangeConfiguration instead.
  context.subscriptions.push(
    onDidChangeConfiguration(async (e: vscode.ConfigurationChangeEvent) => {
      if (e.affectsConfiguration(`${serverId}.enable`)) {
        vscode.window.showWarningMessage(
          `To enable or disable ${LOG_CHANNEL_NAME} after changing the \`enable\` setting, you must restart VS Code.`,
        );
        return;
      }

      const oldSettings = settings;
      const newSettings = getWorkspaceSettings(serverId, {
        uri: vscode.Uri.file(process.cwd()),
        index: 0,
        name: "root",
      } as vscode.WorkspaceFolder);

      if (checkIfConfigurationChanged(oldSettings, newSettings)) {
        await requestRestart();
      }
    }),
    // E3: Workspace trust changes respawn because trust affects binary resolution.
    vscode.workspace.onDidGrantWorkspaceTrust(async () => {
      await requestRestart();
    }),
    // Commands
    registerCommand(`${serverId}.restart`, async () => {
      await requestRestart();
    }),
    registerCommand(`${serverId}.showLogs`, () => {
      logger.channel.show();
    }),
    registerCommand(`${serverId}.showServerLogs`, () => {
      outputChannel.show();
    }),
    registerCommand(`${serverId}.debugInformation`, async () => {
      await debugInformationCommand(resolvedBinary ?? undefined, settings);
    }),
    registerCommand(`${serverId}.explainRule`, async () => {
      if (resolvedBinary) {
        await explainRuleCommand(resolvedBinary.path);
      }
    }),
  );

  // Start the server shortly after activation.
  setImmediate(async () => {
    if (serverState == null && restartPromise == null) {
      try {
        await requestRestart();
      } catch (ex) {
        logger.error(`Failed to start the ${LOG_CHANNEL_NAME} server: ${ex}`);
      }
    }
  });
}

export async function deactivate(): Promise<void> {
  if (restartPromise != null) {
    try {
      await restartPromise;
    } catch {
      // A failed start leaves nothing to stop.
    }
  }
  if (serverState != null) {
    await stopServer(serverState.client);
    serverState = null;
  }
}
