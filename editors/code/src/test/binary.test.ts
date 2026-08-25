/**
 * Unit tests for binary resolution trust behavior.
 *
 * `findRyBinaryPath()` honors workspace trust: an untrusted workspace
 * must NOT use a `ry.path` setting, even if the file exists — that
 * would allow a checked-in `.vscode/settings.json` to execute an
 * arbitrary binary.
 */

import { describe, it, expect } from "bun:test";
import { findRyBinaryPath } from "../common/binary";
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
    } as unknown as import("../common/settings").ISettings;

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
    } as unknown as import("../common/settings").ISettings;

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
    } as unknown as import("../common/settings").ISettings;

    const resolved = findRyBinaryPath(settings, false);
    expect(resolved).toBe(BUNDLED_RY_EXECUTABLE);
  });
});
