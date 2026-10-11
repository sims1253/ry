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
mock.module("vscode-languageclient", () => ({ State: {} }));
mock.module("vscode-languageclient/node", () => ({
  RevealOutputChannelOn: { Never: 0 },
  LanguageClient: class {
    onDidChangeState() {
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

const { startServer } = await import("../common/server");

describe("server start failure", () => {
  it("names the binary and reason, and disposes the client", async () => {
    for (const reason of [new Error("spawn ENOENT"), "spawn ENOENT"]) {
      disposed = 0;
      startFailure = reason;
      stopFailure = undefined;
      await expect(
        startServer("ry", "/missing/ry", {} as never, {} as never),
      ).rejects.toThrow("Server failed to start at /missing/ry: spawn ENOENT");
      expect(disposed).toBe(1);
    }
  });

  it("logs a cleanup failure without hiding the startup failure", async () => {
    logs.length = 0;
    startFailure = new Error("initialization failed");
    stopFailure = new Error("cleanup failed");
    await expect(
      startServer("ry", "/broken/ry", {} as never, {} as never),
    ).rejects.toThrow("initialization failed");
    expect(logs.join("\n")).toContain("cleanup failed");
  });
});
