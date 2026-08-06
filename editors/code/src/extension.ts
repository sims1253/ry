import * as vscode from "vscode";
import { LOG_CHANNEL_NAME, RY_SETTINGS_NAMESPACE } from "./common/constants";
import { LazyOutputChannel, logger } from "./common/logger";
import { type ServerState, startServer, stopServer } from "./common/server";
import {
  getConfiguration,
  onDidChangeConfiguration,
  registerCommand,
} from "./common/vscodeapi";

let serverState: ServerState | null = null;
let restartQueued = false;
let restartPromise: Promise<void> | null = null;

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  const serverId = RY_SETTINGS_NAMESPACE;

  logger.info(`Name: ${LOG_CHANNEL_NAME}`);
  logger.info(`Module: ${serverId}`);

  // Three output channels: the extension's own log (the logger channel),
  // the server's stderr, and the LSP trace (lazily created).
  const outputChannel = vscode.window.createOutputChannel(
    `${LOG_CHANNEL_NAME} Language Server`,
  );
  const traceOutputChannel = new LazyOutputChannel(
    `${LOG_CHANNEL_NAME} Language Server Trace`,
  );

  context.subscriptions.push(outputChannel);
  context.subscriptions.push(traceOutputChannel);
  context.subscriptions.push(logger.channel);

  context.subscriptions.push(
    onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration(`${serverId}.enable`)) {
        vscode.window.showWarningMessage(
          `To enable or disable ${LOG_CHANNEL_NAME} after changing the \`enable\` setting, you must restart VS Code.`,
        );
      }
    }),
  );

  // The `ry.enable` gate: return early from activation when disabled.
  const { enable } = getConfiguration(serverId) as unknown as {
    enable: boolean;
  };
  if (!enable) {
    logger.info(
      `Extension is disabled. To enable, change \`${serverId}.enable\` to \`true\` and restart VS Code.`,
    );
    return;
  }

  const runServer = async () => {
    if (serverState != null) {
      await stopServer(serverState.client);
      serverState = null;
    }

    serverState = await startServer(
      serverId,
      outputChannel,
      traceOutputChannel,
    );
  };

  // Restart orchestration (ruff-vscode's coalescing pattern): a restart
  // requested while one is in flight sets the flag rather than starting a
  // second, and the in-flight restart loops once more when it finishes. At
  // most one restart runs, at most one pends. This is the basic form for
  // E1; E3 wires it to configuration/trust/command triggers.
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
        // Reset the promise after success, an early return, or an error.
        restartPromise = null;
      }
    })();
    await restartPromise;
  };

  context.subscriptions.push(
    onDidChangeConfiguration(async (e: vscode.ConfigurationChangeEvent) => {
      // Only settings that need a respawn trigger a restart. The full
      // `checkIfConfigurationChanged` filter arrives with E3; for now a
      // change to `ry.path` respawns.
      if (e.affectsConfiguration(`${serverId}.path`)) {
        await requestRestart();
      }
    }),
    registerCommand(`${serverId}.showLogs`, () => {
      logger.channel.show();
    }),
    registerCommand(`${serverId}.showServerLogs`, () => {
      outputChannel.show();
    }),
    registerCommand(`${serverId}.restart`, async () => {
      await requestRestart();
    }),
  );

  // Start the server shortly after activation so it is not on the
  // activation call's critical path.
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
