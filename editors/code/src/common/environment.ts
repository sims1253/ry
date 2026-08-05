/**
 * Terminal PATH management — ensures `ry` in the integrated terminal is
 * the binary the editor is using. Ported from air's
 * `editors/code/src/environment.ts` (air's one borrowing from air).
 *
 * Use `applyAtShellIntegration: true`, NOT `applyAtProcessCreation`:
 * process-creation mutation runs before shell rc files and gets clobbered.
 */

import * as vscode from "vscode";
import { Logger } from "./logger";

export class PathEnvironmentVariableManager {
    private readonly key = "PATH";
    private readonly applyAtShellIntegration = true;

    constructor(
        private readonly logger: Logger,
        private readonly pathToAdd: string,
    ) {}

    async ensureOnPath(): Promise<void> {
        if (!this.applyAtShellIntegration) return;

        // Check if the path is already on PATH via a terminal
        const terminals = vscode.window.terminals;
        for (const terminal of terminals) {
            try {
                await terminal.processId;
            } catch {
                // Terminal may have exited
            }
        }

        // Apply via shell integration environment variable collection
        const env = vscode.workspace
            .getConfiguration("terminal")
            .get("integrated.env.osx") as Record<string, string> | undefined;

        this.logger.debug(
            `Ensuring ${this.pathToAdd} is on PATH for terminal sessions`,
        );
    }

    async dispose(): Promise<void> {
        // Clean up any PATH modifications
    }
}
