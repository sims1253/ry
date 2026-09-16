/**
 * Deferred runtime — the heavyweight extension logic (Effect, the
 * language client, binary resolution) sits behind the dynamic import in
 * `src/extension.ts`, so evaluating these modules happens after
 * activation rather than inside it.
 */

import { Effect } from "effect";
import { causeMessage, errorMessage } from "./errors";
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
 * first server start (deferred one more tick, as before).
 */
export function start(context: vscode.ExtensionContext): Runtime {
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
  context.subscriptions.push(logger);

  // Status item shows the resolved binary path and version.
  const statusItem = new StatusItem("ry-status");
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
    return disabledRuntime();
  }

  let serverState: LanguageClient | null = null;
  let restartQueued = false;
  let restartPromise: Promise<void> | null = null;
  let resolvedBinary: ResolvedBinary | null = null;
  let settings: ISettings | undefined;
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
    void Effect.runPromise(
      Effect.gen(function* () {
        const action = yield* Effect.tryPromise(() =>
          Promise.resolve(
            vscode.window.showErrorMessage(message, "Show Logs", "Configure"),
          ),
        );
        if (action === "Show Logs") outputChannel.show();
        else if (action === "Configure") {
          yield* Effect.tryPromise(() =>
            Promise.resolve(
              vscode.commands.executeCommand(
                "workbench.action.openSettings",
                "ry.path",
              ),
            ),
          );
        }
      }).pipe(
        Effect.catchAll((error) => Effect.sync(() => logger.error(error))),
      ),
    );
  };
  const runServer = () =>
    Effect.gen(function* () {
      const nextSettings = readSettings();
      const path = findRyBinaryPath(nextSettings, !vscode.workspace.isTrusted);
      const nextBinary = { path, version: yield* getRyVersion(path) };
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
      const nextClient = yield* startServer(
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
        yield* stopServer(previous).pipe(
          Effect.catchAll((error) =>
            Effect.sync(() =>
              reportFailure(
                `Failed to stop the previous server: ${errorMessage(error)}`,
              ),
            ),
          ),
        );
      }
    });

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
    restartPromise = Effect.runPromise(
      Effect.gen(function* () {
        // Publish the in-flight promise before even a synchronous failure completes.
        yield* Effect.yieldNow();
        do {
          restartQueued = false;
          yield* runServer().pipe(
            Effect.catchAllCause((cause) =>
              Effect.sync(() =>
                reportFailure(
                  `Failed to start the ${LOG_CHANNEL_NAME} server: ${causeMessage(cause)}`,
                ),
              ),
            ),
          );
        } while (restartQueued);
      }).pipe(
        Effect.ensuring(
          Effect.sync(() => {
            restartPromise = null;
          }),
        ),
      ),
    );
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

  // Start the server shortly after boot.
  setImmediate(() => {
    if (serverState == null && restartPromise == null) {
      void requestRestart();
    }
  });

  return {
    requestRestart,
    showLogs: () => logger.channel.show(),
    showServerLogs: () => outputChannel.show(),
    debugInformation: () =>
      Effect.runPromise(
        debugInformationCommand(resolvedBinary ?? undefined, settings),
      ),
    explainRule: () =>
      resolvedBinary
        ? Effect.runPromise(explainRuleCommand(resolvedBinary.path))
        : Promise.resolve(),
    shutdown: () =>
      Effect.runPromise(
        Effect.gen(function* () {
          const pendingRestart = restartPromise;
          if (pendingRestart != null) {
            yield* Effect.tryPromise(() => pendingRestart).pipe(
              Effect.catchAll(() => Effect.void),
            );
          }
          if (serverState != null) {
            yield* stopServer(serverState);
            serverState = null;
          }
        }),
      ),
  };
}
