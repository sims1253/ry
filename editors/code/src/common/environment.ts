/**
 * Terminal PATH management — ensures `ry` in the integrated terminal is
 * the binary the editor is using. Ported from posit-dev/air's
 * `editors/code/src/environment.ts`.
 *
 * Use `applyAtShellIntegration: true`, NOT `applyAtProcessCreation`:
 * process-creation mutation runs before shell rc files and gets clobbered.
 */

import * as vscode from "vscode";
import { Logger } from "./logger";

// TODO: This is a placeholder. The real implementation requires a
// `vscode.GlobalEnvironmentVariableCollection` passed through from the
// extension activation context. It will be wired up when the extension
// is first tested in a real editor host.
export class PathEnvironmentVariableManager {
  constructor(
    private readonly logger: Logger,
    private readonly pathToAdd: string,
  ) {}

  async ensureOnPath(
    collection?: vscode.GlobalEnvironmentVariableCollection,
  ): Promise<void> {
    if (!collection) {
      this.logger.debug(
        "No environment variable collection available; skipping PATH update",
      );
      return;
    }

    const currentPath = collection.get("PATH")?.value ?? process.env.PATH ?? "";
    if (currentPath.includes(this.pathToAdd)) {
      this.logger.debug(`${this.pathToAdd} is already on PATH`);
      return;
    }

    const newPath = `${this.pathToAdd}${process.platform === "win32" ? ";" : ":"}${currentPath}`;
    collection.prepend("PATH", newPath, {
      applyAtShellIntegration: true,
      applyAtProcessCreation: false,
    });
    this.logger.info(`Prepended ${this.pathToAdd} to terminal PATH`);
  }

  async dispose(
    collection?: vscode.GlobalEnvironmentVariableCollection,
  ): Promise<void> {
    collection?.delete("PATH");
  }
}
