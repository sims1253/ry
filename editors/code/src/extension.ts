/** Load the language client after activation, on deferred boot or a command. */

import * as vscode from "vscode";
import { RY_SETTINGS_NAMESPACE } from "./common/constants";
import type { Runtime } from "./common/runtime";

let runtimePromise: Promise<Runtime> | null = null;
let bootImmediate: NodeJS.Immediate | null = null;
// Set from deactivate() until the next activate(): a command queued during
// teardown must not boot a runtime nothing will shut down.
let deactivated = false;

function runtime(context: vscode.ExtensionContext): Promise<Runtime> {
  if (deactivated) {
    return Promise.reject(new Error("The ry extension is shutting down."));
  }
  if (runtimePromise == null) {
    const pending: Promise<Runtime> = import("./common/runtime")
      .then((module) => module.start(context))
      .catch((error) => {
        // Do not cache the rejection: a later command or deactivate()
        // retries the load instead of failing permanently. A load that
        // deactivate() already dropped must not clear a newer one.
        if (runtimePromise === pending) runtimePromise = null;
        throw error;
      });
    runtimePromise = pending;
  }
  return runtimePromise;
}

export function activate(context: vscode.ExtensionContext): void {
  deactivated = false;
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
  deactivated = true;
  // Cancel a still-pending boot so it cannot load the runtime after
  // deactivation.
  if (bootImmediate != null) {
    clearImmediate(bootImmediate);
    bootImmediate = null;
  }
  // Take the load so a second deactivate() cannot shut it down again.
  const active = runtimePromise;
  runtimePromise = null;
  return active == null
    ? Promise.resolve()
    : active.then(
        (instance) => instance.shutdown(),
        // A failed runtime load leaves nothing to shut down.
        () => {},
      );
}
