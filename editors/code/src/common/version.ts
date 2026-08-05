/**
 * Version comparison and capability gating.
 *
 * Ported from ruff-vscode's `src/common/version.ts`.
 * ry is better positioned than ruff here: `ry version --output-format json`
 * emits structured JSON, so no text parsing is needed.
 */

export interface VersionInfo {
    major: number;
    minor: number;
    patch: number;
}

export function versionToString(v: VersionInfo): string {
    return `${v.major}.${v.minor}.${v.patch}`;
}

export function versionFromString(s: string): VersionInfo | undefined {
    const parts = s.split(".").map(Number);
    if (parts.length < 3 || parts.some(isNaN)) {
        return undefined;
    }
    return { major: parts[0], minor: parts[1], patch: parts[2] };
}

export function versionGte(a: VersionInfo, b: VersionInfo): boolean {
    if (a.major !== b.major) return a.major > b.major;
    if (a.minor !== b.minor) return a.minor > b.minor;
    return a.patch >= b.patch;
}

/**
 * The minimum server version that supports the initializationOptions
 * settings channel (S2). Older binaries will produce an actionable
 * error message instead of launching.
 */
export const MINIMUM_SETTINGS_CHANNEL_VERSION: VersionInfo = {
    major: 0,
    minor: 8,
    patch: 0,
};
