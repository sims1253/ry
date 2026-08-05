/**
 * Binary resolution — decides which `ry` executable to run and probes
 * its version before launching the server.
 *
 * Resolution order (from ruff-vscode, minus the Python-interpreter
 * machinery which has no R analogue):
 *
 * 1. `ry.path` entries (first existing wins)
 * 2. `ry.importStrategy == "fromEnvironment"`: `PATH` via `which`
 * 3. Bundled binary (`bundled/bin/ry`)
 *
 * Untrusted workspaces force the bundled binary, ignoring both `path`
 * and `importStrategy`.
 */

import * as path from "path";
import * as cp from "child_process";
import * as fs from "fs";
import { Logger } from "./logger";
import {
    BUNDLED_RY_EXECUTABLE,
    EXTENSION_ROOT_DIR,
    RY_BINARY_NAME,
} from "./constants";
import { VersionInfo, versionFromString, versionGte, MINIMUM_SETTINGS_CHANNEL_VERSION, versionToString } from "./version";
import { ISettings } from "./settings";
import * as vscode from "vscode";

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

    // 1. User-specified path entries (first existing wins)
    for (const candidate of settings.path ?? []) {
        const expanded = resolveHomeDir(candidate);
        if (fs.existsSync(expanded)) {
            return expanded;
        }
    }

    // 2. fromEnvironment: check PATH
    if (settings.importStrategy !== "useBundled") {
        const onPath = findOnPath(RY_BINARY_NAME);
        if (onPath) {
            return onPath;
        }
    }

    // 3. Bundled binary
    return BUNDLED_RY_EXECUTABLE;
}

function resolveHomeDir(p: string): string {
    if (p.startsWith("~/") || p === "~") {
        return path.join(process.env.HOME ?? process.env.USERPROFILE ?? "", p.slice(1));
    }
    return p;
}

function findOnPath(binary: string): string | undefined {
    const sep = process.platform === "win32" ? ";" : ":";
    const pathDirs = (process.env.PATH ?? "").split(sep);
    for (const dir of pathDirs) {
        const candidate = path.join(dir, binary);
        if (fs.existsSync(candidate)) {
            return candidate;
        }
    }
    return undefined;
}

/**
 * Probe the ry binary version by executing `ry version --output-format json`.
 * Returns undefined if the binary cannot be executed or the output
 * cannot be parsed.
 */
export function getRyVersion(binaryPath: string): VersionInfo | undefined {
    try {
        const output = cp.execSync(`"${binaryPath}" version --output-format json`, {
            encoding: "utf-8",
            timeout: 5000,
            stdio: ["pipe", "pipe", "pipe"],
        });
        const parsed = JSON.parse(output);
        const version = parsed.version as string | undefined;
        if (!version) return undefined;
        return versionFromString(version);
    } catch {
        return undefined;
    }
}

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
        return `Could not determine the version of ry at ${binary.path}. ${capabilityName} requires version ${versionToString(minimum)} or later.`;
    }
    if (!versionGte(binary.version, minimum)) {
        return `Found ry version ${versionToString(binary.version)} at ${binary.path}. ${capabilityName} requires version ${versionToString(minimum)} or later. Please update ry.`;
    }
    return undefined;
}

export const MINIMUM_VERSION = MINIMUM_SETTINGS_CHANNEL_VERSION;
