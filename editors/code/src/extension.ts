import * as vscode from "vscode";
import { LOG_CHANNEL_NAME, RY_SETTINGS_NAMESPACE } from "./common/constants";
import { LazyOutputChannel, logger } from "./common/logger";
import { startServer, stopServer } from "./common/server";
import type { LanguageClient } from "vscode-languageclient/node";
import {
  getWorkspaceSettings,
  getGlobalSettings,
  type ISettings,
  checkIfConfigurationChanged,
} from "./common/settings";
import {
  findRyBinaryPath,
  getRyVersion,
  checkVersionCapability,
  type ResolvedBinary,
} from "./common/binary";
import { MINIMUM_SETTINGS_CHANNEL_VERSION } from "./common/version";
import { StatusItem } from "./common/status";
import { debugInformationCommand, explainRuleCommand } from "./common/commands";

let serverState: LanguageClient | null = null;
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

  const enable = vscode.workspace
    .getConfiguration(serverId)
    .get<boolean>("enable", true);
  if (!enable) {
    logger.info(
      `Extension is disabled. To enable, change \`${serverId}.enable\` to \`true\` and restart VS Code.`,
    );
    statusItem.setWarning("Extension disabled");
    return;
  }

  let settings: ISettings | undefined;
  const readSettings = () => {
    const folder = vscode.workspace.workspaceFolders?.[0];
    return folder
      ? getWorkspaceSettings(serverId, folder)
      : getGlobalSettings(serverId);
  };
  const reportFailure = (message: string) => {
    logger.error(message);
    if (serverState && resolvedBinary) statusItem?.setReady(resolvedBinary);
    else statusItem?.setError(message);
    void vscode.window
      .showErrorMessage(message, "Show Logs", "Configure")
      .then((action) => {
        if (action === "Show Logs") outputChannel.show();
        else if (action === "Configure")
          void vscode.commands.executeCommand(
            "workbench.action.openSettings",
            "ry.path",
          );
      });
  };
  const runServer = async () => {
    const nextSettings = readSettings();
    const path = findRyBinaryPath(nextSettings, !vscode.workspace.isTrusted);
    const nextBinary = { path, version: await getRyVersion(path) };
    const error = checkVersionCapability(
      nextBinary,
      MINIMUM_SETTINGS_CHANNEL_VERSION,
      "settings channel",
    );
    if (error) {
      reportFailure(error);
      return;
    }

    statusItem?.setBusy();
    const nextClient = await startServer(
      serverId,
      path,
      outputChannel,
      traceOutputChannel,
    );
    if (!nextClient) {
      reportFailure(`Server failed to start at ${path}`);
      return;
    }

    const previous = serverState;
    serverState = nextClient;
    resolvedBinary = nextBinary;
    settings = nextSettings;
    statusItem?.setReady(nextBinary);
    if (previous) {
      try {
        await stopServer(previous);
      } catch (error) {
        reportFailure(`Failed to stop the previous server: ${error}`);
      }
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
          try {
            await runServer();
          } catch (error) {
            reportFailure(
              `Failed to start the ${LOG_CHANNEL_NAME} server: ${error}`,
            );
          }
        } while (restartQueued);
      } finally {
        restartPromise = null;
      }
    })();
    await restartPromise;
  };

  // Configuration change triggers restart only for settings
  // that need a respawn. Live-updatable settings go via
  // didChangeConfiguration instead.
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(
      async (e: vscode.ConfigurationChangeEvent) => {
        if (e.affectsConfiguration(`${serverId}.enable`)) {
          vscode.window.showWarningMessage(
            `To enable or disable ${LOG_CHANNEL_NAME} after changing the \`enable\` setting, you must restart VS Code.`,
          );
          return;
        }

        const newSettings = readSettings();
        if (!settings || checkIfConfigurationChanged(settings, newSettings)) {
          await requestRestart();
        } else {
          settings = newSettings;
        }
      },
    ),
    // Workspace trust changes respawn because trust affects binary resolution.
    vscode.workspace.onDidGrantWorkspaceTrust(async () => {
      await requestRestart();
    }),
    // Commands
    vscode.commands.registerCommand(`${serverId}.restart`, async () => {
      await requestRestart();
    }),
    vscode.commands.registerCommand(`${serverId}.showLogs`, () => {
      logger.channel.show();
    }),
    vscode.commands.registerCommand(`${serverId}.showServerLogs`, () => {
      outputChannel.show();
    }),
    vscode.commands.registerCommand(
      `${serverId}.debugInformation`,
      async () => {
        await debugInformationCommand(resolvedBinary ?? undefined, settings);
      },
    ),
    vscode.commands.registerCommand(`${serverId}.explainRule`, async () => {
      if (resolvedBinary) {
        await explainRuleCommand(resolvedBinary.path);
      }
    }),
  );

  // Start the server shortly after activation.
  setImmediate(async () => {
    if (serverState == null && restartPromise == null) {
      await requestRestart();
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
    await stopServer(serverState);
    serverState = null;
  }
}
