/**
 * Language status item — shows the resolved binary path, version, and
 * a busy spinner during startup. Ported from ruff-vscode's
 * `src/common/status.ts`.
 */

import * as vscode from "vscode";
import { ResolvedBinary } from "./binary";

export class StatusItem {
    private readonly item: vscode.LanguageStatusItem;

    constructor(id: string) {
        this.item = vscode.languages.createLanguageStatusItem(id, {
            language: "r",
        });
        this.item.name = "ry";
        this.item.severity = vscode.LanguageStatusSeverity.Information;
    }

    setBusy(): void {
        this.item.busy = true;
        this.item.text = "ry: starting…";
        this.item.detail = undefined;
    }

    setReady(binary: ResolvedBinary): void {
        this.item.busy = false;
        this.item.severity = vscode.LanguageStatusSeverity.Information;
        this.item.text = `ry ${binary.version ? `${binary.version.major}.${binary.version.minor}.${binary.version.patch}` : "unknown"}`;
        this.item.detail = binary.path;
    }

    setError(message: string): void {
        this.item.busy = false;
        this.item.severity = vscode.LanguageStatusSeverity.Error;
        this.item.text = "ry: error";
        this.item.detail = message;
    }

    setWarning(message: string): void {
        this.item.busy = false;
        this.item.severity = vscode.LanguageStatusSeverity.Warning;
        this.item.text = "ry: warning";
        this.item.detail = message;
    }

    setCommand(command: string, title: string): void {
        this.item.command = { command, title, arguments: [] };
    }

    dispose(): void {
        this.item.dispose();
    }
}
