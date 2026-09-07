import { Effect, Either } from "effect";
import { runBinary } from "../common/process";
/**
 * Unit tests for binary resolution trust behavior.
 *
 * `findRyBinaryPath()` honors workspace trust: an untrusted workspace
 * must NOT use a `ry.path` setting, even if the file exists — that
 * would allow a checked-in `.vscode/settings.json` to execute an
 * arbitrary binary.
 */

import type { ISettings } from "../common/settings";
import { describe, it, expect } from "bun:test";
import { findRyBinaryPath, getRyVersion } from "../common/binary";
import { BUNDLED_RY_EXECUTABLE } from "../common/constants";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

describe("findRyBinaryPath trust behavior", () => {
  it("ignores ry.path in an untrusted workspace and returns the bundled binary", () => {
    // Create a decoy binary that exists on disk.
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "ry-test-"));
    const decoyPath = path.join(tmpDir, "decoy-ry");
    fs.writeFileSync(decoyPath, "#!/bin/sh\necho decoy\n");
    fs.chmodSync(decoyPath, 0o755);

    const settings = {
      path: [decoyPath],
      importStrategy: "useBundled" as const,
      lint: {},
    } satisfies ISettings;

    // Untrusted: must return the bundled binary, NOT the decoy.
    const resolved = findRyBinaryPath(settings, true);
    expect(resolved).toBe(BUNDLED_RY_EXECUTABLE);
    expect(resolved).not.toBe(decoyPath);

    // Cleanup
    fs.unlinkSync(decoyPath);
    fs.rmdirSync(tmpDir);
  });

  it("uses ry.path in a trusted workspace when the file exists", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "ry-test-"));
    const decoyPath = path.join(tmpDir, "decoy-ry");
    fs.writeFileSync(decoyPath, "#!/bin/sh\necho decoy\n");
    fs.chmodSync(decoyPath, 0o755);

    const settings = {
      path: [decoyPath],
      importStrategy: "useBundled" as const,
      lint: {},
    } satisfies ISettings;

    // Trusted: should use the decoy from ry.path.
    const resolved = findRyBinaryPath(settings, false);
    expect(resolved).toBe(decoyPath);

    // Cleanup
    fs.unlinkSync(decoyPath);
    fs.rmdirSync(tmpDir);
  });

  it("falls back to bundled binary when ry.path does not exist in a trusted workspace", () => {
    const settings = {
      path: ["/nonexistent/decoy-ry"],
      importStrategy: "useBundled" as const,
      lint: {},
    } satisfies ISettings;

    const resolved = findRyBinaryPath(settings, false);
    expect(resolved).toBe(BUNDLED_RY_EXECUTABLE);
  });
});

it("skips directories and non-executable files before a runnable candidate", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ry-candidates-"));
  try {
    const plain = path.join(dir, "plain");
    const executable = path.join(dir, "executable");
    fs.writeFileSync(plain, "not executable", { mode: 0o644 });
    fs.writeFileSync(executable, "#!/bin/sh\n", { mode: 0o755 });
    const settings = {
      path: [dir, ...(process.platform === "win32" ? [] : [plain]), executable],
      importStrategy: "useBundled",
      lint: {},
    } satisfies ISettings;
    expect(findRyBinaryPath(settings, false)).toBe(executable);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

it.skipIf(process.platform === "win32")(
  "version probing leaves the event loop responsive",
  async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ry-version-"));
    try {
      const binary = path.join(dir, "ry");
      fs.writeFileSync(
        binary,
        `#!/bin/sh\nsleep 0.1\necho '{"version":"0.9.0"}'\n`,
        { mode: 0o755 },
      );
      let resolved = false;
      const probe = Effect.runPromise(getRyVersion(binary)).then((version) => {
        resolved = true;
        return version;
      });
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(resolved).toBe(false);
      expect(await probe).toEqual({ major: 0, minor: 9, patch: 0 });
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  },
);

it.skipIf(process.platform === "win32")(
  "version probing rejects malformed and non-string responses",
  async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ry-version-json-"));
    try {
      const binary = path.join(dir, "ry");
      for (const output of [
        "not JSON",
        "null",
        "[]",
        "{}",
        '{"version": 9}',
        '{"version": ["0.9.0"]}',
      ]) {
        fs.writeFileSync(binary, `#!/bin/sh\nprintf '%s\\n' '${output}'\n`, {
          mode: 0o755,
        });
        expect(await Effect.runPromise(getRyVersion(binary))).toBeUndefined();
      }
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  },
);

it("returns an unknown version when the executable is missing", async () => {
  expect(
    await Effect.runPromise(getRyVersion("/nonexistent/ry")),
  ).toBeUndefined();
});

it("keeps process failures in the typed error channel", async () => {
  const result = await Effect.runPromise(
    runBinary("/nonexistent/ry", []).pipe(Effect.either),
  );
  expect(Either.isLeft(result)).toBe(true);
  if (Either.isLeft(result)) {
    expect(result.left._tag).toBe("ProcessError");
    expect(result.left.binaryPath).toBe("/nonexistent/ry");
    expect(String(result.left)).toContain("ENOENT");
    expect(String(result.left)).toContain("/nonexistent/ry");
  }
});

it.skipIf(process.platform === "win32")(
  "CLI effects are lazy and reusable, with cached version probes",
  async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ry-effect-"));
    try {
      const binary = path.join(dir, "ry");
      const marker = path.join(dir, "invocations");
      fs.writeFileSync(
        binary,
        `#!/bin/sh\necho run >> "$0.marker"\necho '{"version":"0.9.0"}'\n`,
        { mode: 0o755 },
      );
      const probe = getRyVersion(binary);
      expect(fs.existsSync(binary + ".marker")).toBe(false);
      await Effect.runPromise(probe);
      // Re-running against an unchanged binary is served from the cache.
      await Effect.runPromise(probe);
      expect(fs.readFileSync(binary + ".marker", "utf8")).toBe("run\n");
      // Replacing the binary (new mtime) invalidates the cache entry.
      const later = new Date(Date.now() + 2000);
      fs.utimesSync(binary, later, later);
      await Effect.runPromise(probe);
      fs.renameSync(binary + ".marker", marker);
      expect(fs.readFileSync(marker, "utf8")).toBe("run\nrun\n");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  },
);
