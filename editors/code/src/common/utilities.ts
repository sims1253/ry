import type { DocumentSelector } from "vscode-languageclient";
import { isVirtualWorkspace } from "./vscodeapi";

/**
 * The document selector for files the server should manage.
 *
 * Under a virtual workspace (github.dev, vscode.dev) the `file` scheme
 * is dropped entirely, since the server only operates on local files;
 * without this the extension is silently inert there. Otherwise the
 * `file`, `untitled`, and notebook schemes are enumerated explicitly.
 *
 * `ry.toml` is included so that config edits arrive as ordinary
 * `didOpen`/`didChange` syncs, complementing `didChangeWatchedFiles`.
 */
export function getDocumentSelector(): DocumentSelector {
  return isVirtualWorkspace()
    ? [{ language: "r" }]
    : [
        { scheme: "file", language: "r" },
        { scheme: "untitled", language: "r" },
        { scheme: "vscode-notebook", language: "r" },
        { scheme: "vscode-notebook-cell", language: "r" },
        { scheme: "file", pattern: "**/{ry.toml}" },
      ];
}
