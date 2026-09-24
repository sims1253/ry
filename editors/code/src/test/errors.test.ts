// Partial VS Code doubles expose only the methods these failure paths use.
/* oxlint-disable typescript/consistent-type-assertions */
import { describe, expect, it, mock } from "bun:test";

const logs: string[] = [];
let startFailure: unknown;
let stopFailure: unknown;
let disposed = 0;

mock.module("vscode", () => ({
  workspace: { getConfiguration: () => ({ get: () => undefined }) },
}));
mock.module("../common/logger", () => ({
  logger: {
    info() {},
    debug() {},
    error: (message: string) => logs.push(message),
  },
}));
mock.module("../common/settings", () => ({
  getExtensionSettings: () => [],
  getGlobalSettings: () => ({ lint: {} }),
  getWorkspaceSettings: () => ({ lint: {} }),
}));
mock.module("vscode-languageclient", () => ({
  MessageType: {},
  ShowMessageNotification: { type: {} },
  State: {},
}));
mock.module("vscode-languageclient/node", () => ({
  RevealOutputChannelOn: { Never: 0 },
  LanguageClient: class {
    onDidChangeState() {
      return { dispose() {} };
    }
    onNotification() {
      return { dispose() {} };
    }
    start() {
      return Promise.reject(startFailure);
    }
    dispose() {
      disposed++;
      return stopFailure ? Promise.reject(stopFailure) : Promise.resolve();
    }
  },
}));

const { errorMessage } = await import("../common/errors");

const { startServer, stopServer } = await import("../common/server");

describe("server failure details", () => {
  it("preserves the start reason and binary path in the error and log", async () => {
    logs.length = 0;
    disposed = 0;
    startFailure = new Error("spawn ENOENT");
    stopFailure = undefined;
    await expect(
      startServer("ry", "/missing/ry", {} as never, {} as never),
    ).rejects.toThrow("Server failed to start at /missing/ry: spawn ENOENT");
    expect(logs.join("\n")).toContain("spawn ENOENT");
    expect(disposed).toBe(1);
  });

  it("preserves a stop rejection", async () => {
    const client = {
      dispose: () => Promise.reject(new Error("shutdown failed")),
    };
    await expect(stopServer(client as never)).rejects.toThrow(
      "shutdown failed",
    );
  });
});

it("preserves error messages and string rejections", () => {
  for (const reason of [
    new Error("document unavailable"),
    "document unavailable",
  ]) {
    expect(errorMessage(reason)).toBe("document unavailable");
  }
});

it("logs cleanup failure without hiding the original startup failure", async () => {
  logs.length = 0;
  startFailure = new Error("initialization failed");
  stopFailure = new Error("cleanup failed");
  await expect(
    startServer("ry", "/broken/ry", {} as never, {} as never),
  ).rejects.toThrow("initialization failed");
  expect(logs.join("\n")).toContain("initialization failed");
  expect(logs.join("\n")).toContain("cleanup failed");
});
