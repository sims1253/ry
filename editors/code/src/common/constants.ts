import * as path from "path";

const folderName = path.basename(__dirname);

/**
 * Path to the root directory of this extension.
 *
 * Resolves correctly whether the compiled file lives at `dist/`
 * (bundled, where `__dirname` is the extension root) or at
 * `out/src/common/` (tsc-only, where `__dirname` ends in `common`).
 */
export const EXTENSION_ROOT_DIR =
  folderName === "common"
    ? path.dirname(path.dirname(__dirname))
    : path.dirname(__dirname);

/**
 * Extension ID on the marketplaces (`<publisher>.<name>`).
 */
export const RY_EXTENSION_ID = "ry.ry";

/**
 * The VS Code settings namespace (`ry.*`).
 */
export const RY_SETTINGS_NAMESPACE = "ry";

/**
 * The log channel name used for the extension's own log and for the
 * trace channel labels shown in the VS Code output panel.
 */
export const LOG_CHANNEL_NAME = "ry";

/**
 * Name of the `ry` binary based on the current platform.
 */
export const RY_BINARY_NAME = process.platform === "win32" ? "ry.exe" : "ry";

/**
 * Path to the `ry` executable that is bundled with the extension.
 *
 * CI injects the platform-specific binary here; the directory is
 * gitignored. Binary resolution (E2) picks between this path and a
 * user-installed `ry`.
 */
export const BUNDLED_RY_EXECUTABLE = path.join(
  EXTENSION_ROOT_DIR,
  "bundled",
  "bin",
  RY_BINARY_NAME,
);

/**
 * The subcommand for the `ry` binary that starts the language server.
 */
export const RY_SERVER_SUBCOMMAND = "server";
