import * as fs from "fs/promises";
import * as path from "path";
import { Uri, type WorkspaceFolder } from "vscode";
import type { DocumentSelector } from "vscode-languageclient";
import { getWorkspaceFolders, isVirtualWorkspace } from "./vscodeapi";

/**
 * Returns true if a path exists on disk. Mirrors `fs-extra`'s
 * `pathExists`, kept dependency-free by using the built-in `fs/promises`.
 */
async function pathExists(p: string): Promise<boolean> {
  try {
    await fs.access(p);
    return true;
  } catch {
    return false;
  }
}

/**
 * Pick the single workspace folder to use as the project root.
 *
 * With one folder, that folder wins. With several, the shortest
 * existing folder path is chosen (the top-most, least nested
 * directory). With no folders, falls back to the process working directory.
 *
 * This mirrors ruff-vscode's `getProjectRoot`.
 */
export async function getProjectRoot(): Promise<WorkspaceFolder> {
  const workspaces: readonly WorkspaceFolder[] = getWorkspaceFolders();
  if (workspaces.length === 0) {
    return {
      uri: Uri.file(process.cwd()),
      name: path.basename(process.cwd()),
      index: 0,
    };
  } else if (workspaces.length === 1) {
    return workspaces[0];
  } else {
    let rootWorkspace = workspaces[0];
    let root: string | undefined;
    for (const w of workspaces) {
      if (await pathExists(w.uri.fsPath)) {
        root = w.uri.fsPath;
        rootWorkspace = w;
        break;
      }
    }

    for (const w of workspaces) {
      if (
        root !== undefined &&
        root.length > w.uri.fsPath.length &&
        (await pathExists(w.uri.fsPath))
      ) {
        root = w.uri.fsPath;
        rootWorkspace = w;
      }
    }
    return rootWorkspace;
  }
}

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
