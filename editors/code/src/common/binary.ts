/**
 * Binary resolution — decides which `ry` executable to run and probes
 * its version before launching the server.
 *
 * Resolution order (used by this extension):
 *
 * 1. `ry.path` entries (first executable file wins)
 * 2. `ry.importStrategy == "fromEnvironment"`: `PATH`
 * 3. Bundled binary (`bundled/bin/ry`)
 *
 * Untrusted workspaces force the bundled binary, ignoring both `path`
 * and `importStrategy`.
 */

import * as path from "path";
import { Effect, Schema } from "effect";
import { runBinary } from "./process";
import * as fs from "fs";
import { BUNDLED_RY_EXECUTABLE, RY_BINARY_NAME } from "./constants";
import {
  VersionInfo,
  versionFromString,
  versionGte,
  versionToString,
} from "./version";
import { ISettings } from "./settings";

export interface ResolvedBinary {
  path: string;
  version: VersionInfo | undefined;
}

/**
 * Find the ry binary path based on user settings and workspace trust.
 * Untrusted workspaces always use the bundled binary.
 */
export function findRyBinaryPath(
  settings: ISettings,
  isUntrusted: boolean,
): string {
  // Untrusted workspace: force bundled, ignoring path and importStrategy.
  // A `ry.path` entry in a checked-in `.vscode/settings.json` is
  // arbitrary code execution on folder open.
  if (isUntrusted) {
    return BUNDLED_RY_EXECUTABLE;
  }

  // 1. User-specified path entries (first executable file wins)
  for (const candidate of settings.path ?? []) {
    const expanded = resolveHomeDir(candidate);
    if (isExecutableFile(expanded)) {
      return expanded;
    }
  }

  // 2. fromEnvironment: check PATH
  if (settings.importStrategy === "fromEnvironment") {
    const onPath = findOnPath(RY_BINARY_NAME);
    if (onPath) {
      return onPath;
    }
  }

  // 3. Bundled binary
  return BUNDLED_RY_EXECUTABLE;
}

function isExecutableFile(candidate: string): boolean {
  try {
    if (!fs.statSync(candidate).isFile()) return false;
    fs.accessSync(candidate, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

function resolveHomeDir(p: string): string {
  if (p.startsWith("~/") || p === "~") {
    return path.join(
      process.env.HOME ?? process.env.USERPROFILE ?? "",
      p.slice(1),
    );
  }
  return p;
}

function findOnPath(binary: string): string | undefined {
  const sep = process.platform === "win32" ? ";" : ":";
  const pathDirs = (process.env.PATH ?? "")
    .split(sep)
    .filter((dir) => dir.length > 0);
  for (const dir of pathDirs) {
    const candidate = path.join(dir, binary);
    if (isExecutableFile(candidate)) {
      return candidate;
    }
  }
  return undefined;
}

const decodeVersion = Schema.decodeUnknown(
  Schema.parseJson(Schema.Struct({ version: Schema.String })),
);

/** An unavailable binary or invalid response produces an unknown version. */
export const getRyVersion = (binaryPath: string) =>
  runBinary(binaryPath, ["version", "--output-format", "json"]).pipe(
    Effect.flatMap(({ stdout }) => decodeVersion(stdout)),
    Effect.map(({ version }) => versionFromString(version)),
    Effect.catchAll(() => Effect.succeed(undefined)),
  );

/**
 * Check if the resolved binary meets the minimum version for a capability.
 * Returns an error message string if the check fails, undefined otherwise.
 */
export function checkVersionCapability(
  binary: ResolvedBinary,
  minimum: VersionInfo,
  capabilityName: string,
): string | undefined {
  if (!binary.version) {
    return `Could not determine the version of ry at ${binary.path}. The ${capabilityName} requires version ${versionToString(minimum)} or later.`;
  }
  if (!versionGte(binary.version, minimum)) {
    return `Found ry version ${versionToString(binary.version)} at ${binary.path}. The ${capabilityName} requires version ${versionToString(minimum)} or later. Please update ry.`;
  }
  return undefined;
}
