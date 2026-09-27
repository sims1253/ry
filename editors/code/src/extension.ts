/** Load the language client after activation, on deferred boot or a command. */

import * as vscode from "vscode";
import { RY_SETTINGS_NAMESPACE } from "./common/constants";
import type { Runtime } from "./common/runtime";

let runtimePromise: Promise<Runtime> | null = null;
let bootImmediate: NodeJS.Immediate | null = null;

function runtime(context: vscode.ExtensionContext): Promise<Runtime> {
  if (runtimePromise == null) {
    runtimePromise = import("./common/runtime")
      .then((module) => module.start(context))
      .catch((error) => {
        // Do not cache the rejection: a later command or deactivate()
        // retries the load instead of failing permanently.
        runtimePromise = null;
        throw error;
      });
  }
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
  bootImmediate = setImmediate(() => {
    bootImmediate = null;
    void runtime(context).catch((error) => {
      console.error("Failed to initialize the ry runtime.", error);
      void vscode.window.showErrorMessage(
        "Failed to initialize the ry extension.",
      );
    });
  });
}

export function deactivate(): Promise<void> {
  // Cancel a still-pending boot so it cannot load the runtime after
  // deactivation.
  if (bootImmediate != null) {
    clearImmediate(bootImmediate);
    bootImmediate = null;
  }
  return runtimePromise == null
    ? Promise.resolve()
    : runtimePromise.then(
        (instance) => instance.shutdown(),
        // A failed runtime load leaves nothing to shut down.
        () => {},
      );
}
