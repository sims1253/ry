import { Effect } from "effect";
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

      statusItem?.setBusy();
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
      statusItem?.setReady(nextBinary);
      if (previous) {
        yield* stopServer(previous).pipe(
          Effect.catchAll((error) =>
            Effect.sync(() =>
              reportFailure(`Failed to stop the previous server: ${error}`),
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
                  `Failed to start the ${LOG_CHANNEL_NAME} server: ${cause}`,
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
        await Effect.runPromise(
          debugInformationCommand(resolvedBinary ?? undefined, settings),
        );
      },
    ),
    vscode.commands.registerCommand(`${serverId}.explainRule`, async () => {
      if (resolvedBinary) {
        await Effect.runPromise(explainRuleCommand(resolvedBinary.path));
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

export function deactivate(): Promise<void> {
  return Effect.runPromise(
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
  );
}
