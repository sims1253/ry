// Partial VS Code doubles expose only the methods the runtime boot uses.
/* oxlint-disable typescript/consistent-type-assertions */
import { describe, expect, it, mock } from "bun:test";

type ConfigListener = (event: {
  affectsConfiguration(section: string): boolean;
}) => Promise<void>;

let configListener: ConfigListener | undefined;
let currentPath: string[] = [];
let starts = 0;
// Each test decides whether the next start hangs until it settles it.
let holdNextStart = false;
let releaseFirstStart: (() => void) | undefined;
let failHeldStart: ((error: unknown) => void) | undefined;
let toasts = 0;

const disposable = { dispose() {} };
const channel = { show() {}, dispose() {} };

mock.module("vscode", () => ({
  workspace: {
    workspaceFolders: undefined,
    isTrusted: true,
    getConfiguration: () => ({
      get: (_key: string, fallback: unknown) => fallback,
    }),
    onDidChangeConfiguration(listener: ConfigListener) {
      configListener = listener;
      return disposable;
    },
    onDidGrantWorkspaceTrust: () => disposable,
  },
  window: {
    createOutputChannel: () => channel,
    showErrorMessage: () => {
      toasts++;
      return Promise.resolve(undefined);
    },
    showWarningMessage: () => Promise.resolve(undefined),
  },
}));
mock.module("../common/logger", () => ({
  logger: { info() {}, debug() {}, error() {}, channel, dispose() {} },
  LazyOutputChannel: class {
    dispose() {}
  },
}));
mock.module("../common/status", () => ({
  StatusItem: class {
    setBusy() {}
    setReady() {}
    setError() {}
    setWarning() {}
    dispose() {}
  },
}));
mock.module("../common/commands", () => ({
  debugInformationCommand: async () => {},
  explainRuleCommand: async () => {},
}));
mock.module("../common/binary", () => ({
  findRyBinaryPath: () => "/bin/ry",
  getRyVersion: async () => ({ major: 1, minor: 0, patch: 0 }),
  checkVersionCapability: () => undefined,
}));
mock.module("../common/settings", () => ({
  getGlobalSettings: () => ({ path: currentPath, lint: {} }),
  getWorkspaceSettings: () => ({ path: currentPath, lint: {} }),
  checkIfConfigurationChanged: (
    previous: { path: string[] },
    next: { path: string[] },
  ) => JSON.stringify(previous.path) !== JSON.stringify(next.path),
}));
mock.module("../common/server", () => ({
  startServer: () => {
    starts++;
    if (!holdNextStart) return Promise.resolve({});
    holdNextStart = false;
    return new Promise((resolve, reject) => {
      releaseFirstStart = () => resolve({});
      failHeldStart = reject;
    });
  },
  stopServer: async () => {},
}));

const { start } = await import("../common/runtime");

const event = (sections: string[]) => ({
  affectsConfiguration: (section: string) =>
    sections.some((s) => s === section || s.startsWith(`${section}.`)),
});
const settle = () => new Promise((resolve) => setTimeout(resolve, 20));

describe("startup configuration events (#919)", () => {
  it("start one server, then restart only for a changed ry.path", async () => {
    starts = 0;
    // Hold the first start open so startup configuration events land
    // while it is in flight, as they do in a real window.
    holdNextStart = true;
    const runtime = start({ subscriptions: [] } as never);
    await settle();
    expect(starts).toBe(1);

    // Startup noise while the first start is in flight: an unrelated
    // section and an `ry` event that changes nothing restart-relevant.
    // Not awaited: on the old path a queued restart waits on the held start.
    void configListener?.(event(["editor.fontSize"]));
    void configListener?.(event(["ry.lint"]));
    releaseFirstStart?.();
    await settle();
    expect(starts).toBe(1);

    currentPath = ["/opt/ry"];
    await configListener?.(event(["ry.path"]));
    await settle();
    expect(starts).toBe(2);

    await runtime.shutdown();
  });
});

describe("shutdown during a start", () => {
  it("raises no error toast when the in-flight start fails", async () => {
    starts = 0;
    toasts = 0;
    holdNextStart = true;
    const runtime = start({ subscriptions: [] } as never);
    await settle();
    // The start is in flight before shutdown begins.
    expect(starts).toBe(1);
    const shutdown = runtime.shutdown();
    failHeldStart?.(new Error("spawn ENOENT"));
    await shutdown;
    expect(toasts).toBe(0);
  });
});
