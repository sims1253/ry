/** Language client and binary resolution load after extension activation. */

import { errorMessage } from "./errors";
import * as vscode from "vscode";
import { LOG_CHANNEL_NAME, RY_SETTINGS_NAMESPACE } from "./constants";
import { LazyOutputChannel, logger } from "./logger";
import { startServer, stopServer } from "./server";
import type { LanguageClient } from "vscode-languageclient/node";
import {
  getWorkspaceSettings,
  getGlobalSettings,
  type ISettings,
  checkIfConfigurationChanged,
} from "./settings";
import {
  findRyBinaryPath,
  getRyVersion,
  checkVersionCapability,
  type ResolvedBinary,
} from "./binary";
import { MINIMUM_SETTINGS_CHANNEL_VERSION } from "./version";
import { StatusItem } from "./status";
import { debugInformationCommand, explainRuleCommand } from "./commands";

/**
 * The handle the activation entry uses to reach the booted runtime.
 */
export interface Runtime {
  requestRestart(): Promise<void>;
  showLogs(): void;
  showServerLogs(): void;
  debugInformation(): Promise<void>;
  explainRule(): Promise<void>;
  shutdown(): Promise<void>;
}

const disabledRuntime = (): Runtime => ({
  async requestRestart() {},
  showLogs() {
    logger.channel.show();
  },
  showServerLogs() {},
  async debugInformation() {},
  async explainRule() {},
  async shutdown() {},
});

/**
 * Boot the extension runtime: channels, status item, listeners, and the
 * first server start on the next tick.
 */
export function start(context: vscode.ExtensionContext): Runtime {
  const serverId = RY_SETTINGS_NAMESPACE;

  const enable = vscode.workspace
    .getConfiguration(serverId)
    .get<boolean>("enable", true);
  if (!enable) {
    // Disabled: create only the status item needed for the warning. No
    // logging or server channels, so the lazy client channel stays
    // uncreated; it appears only if `showLogs` is invoked explicitly,
    // and is then disposed through the logger pushed here.
    const statusItem = new StatusItem("ry-status");
    context.subscriptions.push(statusItem, logger);
    statusItem.setWarning("Extension disabled");
    return disabledRuntime();
  }

  logger.info(`Name: ${LOG_CHANNEL_NAME}`);
  logger.info(`Module: ${serverId}`);

  const outputChannel = vscode.window.createOutputChannel(
    `${LOG_CHANNEL_NAME} Language Server`,
  );
  const traceOutputChannel = new LazyOutputChannel(
    `${LOG_CHANNEL_NAME} Language Server Trace`,
  );

  context.subscriptions.push(outputChannel, traceOutputChannel, logger);

  // Status item shows the resolved binary path and version.
  const statusItem = new StatusItem("ry-status");
  statusItem.setBusy();
  context.subscriptions.push(statusItem);

  let serverState: LanguageClient | null = null;
  let restartQueued = false;
  let restartPromise: Promise<void> | null = null;
  let resolvedBinary: ResolvedBinary | null = null;
  let settings: ISettings | undefined;
  let shutdownStarted = false;
  let bootImmediate: NodeJS.Immediate | null = null;
  const readSettings = () => {
    const folder = vscode.workspace.workspaceFolders?.[0];
    return folder
      ? getWorkspaceSettings(serverId, folder)
      : getGlobalSettings(serverId);
  };
  const reportFailure = (message: string) => {
    logger.error(message);
    if (serverState && resolvedBinary) statusItem.setReady(resolvedBinary);
    else statusItem.setError(message);
    void (async () => {
      try {
        const action = await vscode.window.showErrorMessage(
          message,
          "Show Logs",
          "Configure",
        );
        if (action === "Show Logs") outputChannel.show();
        else if (action === "Configure")
          await vscode.commands.executeCommand(
            "workbench.action.openSettings",
            "ry.path",
          );
      } catch (error) {
        logger.error(errorMessage(error));
      }
    })();
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
    statusItem.setBusy();
    const nextClient = await startServer(
      serverId,
      path,
      outputChannel,
      traceOutputChannel,
    );
    const previous = serverState;
    serverState = nextClient;
    resolvedBinary = nextBinary;
    settings = nextSettings;
    statusItem.setReady(nextBinary);
    if (previous) {
      try {
        await stopServer(previous);
      } catch (error) {
        reportFailure(
          `Failed to stop the previous server: ${errorMessage(error)}`,
        );
      }
    }
  };

  // Restart orchestration: at most one restart runs, at most one pends.
  const requestRestart = async () => {
    if (shutdownStarted) return;
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
      // Assign restartPromise before a synchronous failure can clear it.
      await Promise.resolve();
      try {
        do {
          restartQueued = false;
          try {
            await runServer();
          } catch (error) {
            reportFailure(
              `Failed to start the ${LOG_CHANNEL_NAME} server: ${errorMessage(error)}`,
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
  );

  // Start the server shortly after boot. shutdown() cancels a still
  // pending immediate, and the callback is a no-op once shutdown has
  // begun, so the first start can never recreate state after teardown.
  bootImmediate = setImmediate(() => {
    bootImmediate = null;
    if (shutdownStarted) return;
    if (serverState == null && restartPromise == null) {
      void requestRestart();
    }
  });

  return {
    requestRestart,
    showLogs: () => logger.channel.show(),
    showServerLogs: () => outputChannel.show(),
    debugInformation: () =>
      debugInformationCommand(resolvedBinary ?? undefined, settings),
    explainRule: () =>
      resolvedBinary
        ? explainRuleCommand(resolvedBinary.path)
        : Promise.resolve(),
    shutdown: async () => {
      shutdownStarted = true;
      if (bootImmediate != null) {
        clearImmediate(bootImmediate);
        bootImmediate = null;
      }
      await restartPromise?.catch(() => {});
      if (serverState != null) {
        await stopServer(serverState);
        serverState = null;
      }
    },
  };
}
