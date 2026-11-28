// Partial VS Code doubles expose only what activation uses.
/* oxlint-disable typescript/consistent-type-assertions */
import { beforeEach, expect, it, mock } from "bun:test";

let starts = 0;
// Each test decides whether the next load hangs until it rejects it.
let holdNextLoad = false;
let rejectHeldLoad: ((error: unknown) => void) | undefined;
const commands: Record<string, () => Promise<void>> = {};

mock.module("vscode", () => ({
  commands: {
    registerCommand(id: string, handler: () => Promise<void>) {
      commands[id] = handler;
      return { dispose() {} };
    },
  },
  window: { showErrorMessage: () => Promise.resolve(undefined) },
}));
mock.module("../common/runtime", () => ({
  start: () => {
    starts++;
    if (holdNextLoad) {
      holdNextLoad = false;
      return new Promise((_, reject) => (rejectHeldLoad = reject));
    }
    return { showLogs() {}, shutdown: async () => {} };
  },
}));

const { activate, deactivate } = await import("../extension");
const settle = () => new Promise((resolve) => setTimeout(resolve, 10));

beforeEach(async () => {
  await deactivate();
  starts = 0;
  holdNextLoad = false;
  rejectHeldLoad = undefined;
});

it("a load that deactivate() dropped does not clear a newer load", async () => {
  const context = { subscriptions: [] } as never;
  holdNextLoad = true;
  activate(context);
  await settle();
  expect(starts).toBe(1);
  void deactivate();
  activate(context);
  await settle();
  rejectHeldLoad?.(new Error("old load failed"));
  await settle();
  await commands["ry.showLogs"]?.();
  expect(starts).toBe(2);
});

it("a command queued during deactivate() does nothing", async () => {
  const context = { subscriptions: [] } as never;
  activate(context);
  await settle();
  expect(starts).toBe(1);
  const teardown = deactivate();
  await commands["ry.showLogs"]?.();
  await teardown;
  expect(starts).toBe(1);
});
