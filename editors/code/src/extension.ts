/**
 * Activation entry — kept deliberately small. The language client and
 * Effect are loaded through a dynamic import in `runtime()`, so their
 * module evaluation happens after activation completes instead of
 * inside the activation window. Command handlers and the deferred boot
 * below are the only things that can trigger the load.
 */

import * as vscode from "vscode";
import { RY_SETTINGS_NAMESPACE } from "./common/constants";
import type { Runtime } from "./common/runtime";

let runtimePromise: Promise<Runtime> | null = null;

function runtime(context: vscode.ExtensionContext): Promise<Runtime> {
  runtimePromise ??= import("./common/runtime").then((module) =>
    module.start(context),
  );
  return runtimePromise;
}

export function activate(context: vscode.ExtensionContext): void {
  const serverId = RY_SETTINGS_NAMESPACE;

  context.subscriptions.push(
    vscode.commands.registerCommand(`${serverId}.restart`, async () => {
      await (await runtime(context)).requestRestart();
    }),
    vscode.commands.registerCommand(`${serverId}.showLogs`, async () => {
      (await runtime(context)).showLogs();
    }),
    vscode.commands.registerCommand(`${serverId}.showServerLogs`, async () => {
      (await runtime(context)).showServerLogs();
    }),
    vscode.commands.registerCommand(
      `${serverId}.debugInformation`,
      async () => {
        await (await runtime(context)).debugInformation();
      },
    ),
    vscode.commands.registerCommand(`${serverId}.explainRule`, async () => {
      await (await runtime(context)).explainRule();
    }),
  );

  // Boot the runtime shortly after activation.
  setImmediate(() => {
    void runtime(context);
  });
}

export function deactivate(): Promise<void> {
  return runtimePromise == null
    ? Promise.resolve()
    : runtimePromise.then((instance) => instance.shutdown());
}
