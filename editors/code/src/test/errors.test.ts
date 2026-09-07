// Partial VS Code doubles expose only the methods these failure paths use.
/* oxlint-disable typescript/consistent-type-assertions */
import { describe, expect, it, mock } from "bun:test";
import { Effect, Exit } from "effect";

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

const { causeMessage, errorMessage } = await import("../common/errors");

const { startServer, stopServer } = await import("../common/server");

describe("server failure details", () => {
  it("preserves the start reason and binary path in the error and log", async () => {
    logs.length = 0;
    disposed = 0;
    startFailure = new Error("spawn ENOENT");
    stopFailure = undefined;
    const result = await Effect.runPromiseExit(
      startServer("ry", "/missing/ry", {} as never, {} as never),
    );
    expect(Exit.isFailure(result)).toBe(true);
    if (Exit.isFailure(result)) {
      expect(causeMessage(result.cause)).toContain("spawn ENOENT");
      expect(causeMessage(result.cause)).not.toContain("core-effect");
      expect(causeMessage(result.cause)).toContain("/missing/ry");
    }
    expect(logs.join("\n")).toContain("spawn ENOENT");
    expect(disposed).toBe(1);
  });

  it("preserves a stop rejection", async () => {
    const client = {
      dispose: () => Promise.reject(new Error("shutdown failed")),
    };
    const result = await Effect.runPromiseExit(stopServer(client as never));
    expect(Exit.isFailure(result)).toBe(true);
    if (Exit.isFailure(result))
      expect(causeMessage(result.cause)).toBe("shutdown failed");
  });
});

it("unwraps Promise errors and preserves string rejections", async () => {
  for (const reason of [
    new Error("document unavailable"),
    "document unavailable",
  ]) {
    const message = await Effect.runPromise(
      Effect.tryPromise(() => Promise.reject(reason)).pipe(
        Effect.catchAll((error) => Effect.succeed(errorMessage(error))),
      ),
    );
    expect(message).toBe("document unavailable");
  }
});

it("logs cleanup failure without hiding the original startup failure", async () => {
  logs.length = 0;
  startFailure = new Error("initialization failed");
  stopFailure = new Error("cleanup failed");
  const result = await Effect.runPromiseExit(
    startServer("ry", "/broken/ry", {} as never, {} as never),
  );
  expect(Exit.isFailure(result)).toBe(true);
  if (Exit.isFailure(result))
    expect(causeMessage(result.cause)).toContain("initialization failed");
  expect(logs.join("\n")).toContain("initialization failed");
  expect(logs.join("\n")).toContain("cleanup failed");
});
