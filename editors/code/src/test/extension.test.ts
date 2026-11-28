// Partial VS Code doubles expose only what activation uses.
/* oxlint-disable typescript/consistent-type-assertions */
import { expect, it, mock } from "bun:test";

let starts = 0;
const rejectFirst: Array<(error: unknown) => void> = [];
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
    if (starts === 1)
      return new Promise((_, reject) => rejectFirst.push(reject));
    return { showLogs() {}, shutdown: async () => {} };
  },
}));

const { activate, deactivate } = await import("../extension");
const settle = () => new Promise((resolve) => setTimeout(resolve, 10));

it("a load that deactivate() dropped does not clear a newer load", async () => {
  const context = { subscriptions: [] } as never;
  activate(context);
  await settle();
  void deactivate();
  activate(context);
  await settle();
  rejectFirst[0]?.(new Error("old load failed"));
  await settle();
  await commands["ry.showLogs"]?.();
  expect(starts).toBe(2);
});
