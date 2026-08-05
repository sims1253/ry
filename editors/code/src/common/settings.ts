/**
 * Settings module — mirrors ruff-vscode's `settings.ts`.
 *
 * Provides `ISettings`, `getWorkspaceSettings`, `getGlobalSettings`,
 * `getExtensionSettings` (returning the per-folder array that feeds
 * S2's `initializationOptions`), and `checkIfConfigurationChanged`.
 */

import * as vscode from "vscode";
import { getConfiguration, getWorkspaceFolders } from "./vscodeapi";

export const RY_SETTINGS_NAMESPACE = "ry";

export interface ISettings {
    enable?: boolean;
    path?: string[];
    configuration?: string;
    importStrategy: "fromEnvironment" | "useBundled";
    lint: ILintSettings;
    minConfidence?: "low" | "medium" | "high";
    baseline?: string;
    checkTestFixtures?: boolean;
    logLevel?: string;
    addExecutableToTerminalPath: boolean;
}

export interface ILintSettings {
    select?: string[];
    extendSelect?: string[];
    ignore?: string[];
    error?: string[];
    warn?: string[];
}

export function getWorkspaceSettings(
    namespace: string,
    folder: vscode.WorkspaceFolder,
): ISettings {
    const config = getConfiguration(namespace, folder.uri);
    return {
        enable: config.get<boolean>("enable"),
        path: config.get<string[]>("path"),
        configuration: config.get<string>("configuration"),
        importStrategy: config.get<"fromEnvironment" | "useBundled">(
            "importStrategy",
            "fromEnvironment",
        ),
        lint: {
            select: config.get<string[]>("lint.select"),
            extendSelect: config.get<string[]>("lint.extendSelect"),
            ignore: config.get<string[]>("lint.ignore"),
            error: config.get<string[]>("lint.error"),
            warn: config.get<string[]>("lint.warn"),
        },
        minConfidence: config.get<"low" | "medium" | "high">("minConfidence"),
        baseline: config.get<string>("baseline"),
        checkTestFixtures: config.get<boolean>("checkTestFixtures"),
        logLevel: config.get<string>("logLevel"),
        addExecutableToTerminalPath: config.get<boolean>(
            "addExecutableToTerminalPath",
            true,
        ),
    };
}

export function getGlobalSettings(namespace: string): ISettings {
    const config = getConfiguration(namespace);
    return {
        enable: config.get<boolean>("enable"),
        path: config.get<string[]>("path"),
        configuration: config.get<string>("configuration"),
        importStrategy: config.get<"fromEnvironment" | "useBundled">(
            "importStrategy",
            "fromEnvironment",
        ),
        lint: {
            select: config.get<string[]>("lint.select"),
            extendSelect: config.get<string[]>("lint.extendSelect"),
            ignore: config.get<string[]>("lint.ignore"),
            error: config.get<string[]>("lint.error"),
            warn: config.get<string[]>("lint.warn"),
        },
        minConfidence: config.get<"low" | "medium" | "high">("minConfidence"),
        baseline: config.get<string>("baseline"),
        checkTestFixtures: config.get<boolean>("checkTestFixtures"),
        logLevel: config.get<string>("logLevel"),
        addExecutableToTerminalPath: config.get<boolean>(
            "addExecutableToTerminalPath",
            true,
        ),
    };
}

/**
 * Build the per-folder settings array that feeds S2's
 * `initializationOptions`. This is what the server receives at
 * initialize time.
 */
export function getExtensionSettings(namespace: string): ISettings[] {
    const folders = getWorkspaceFolders();
    if (!folders) return [];
    return folders.map((folder) => getWorkspaceSettings(namespace, folder));
}

/**
 * Determine whether a configuration change requires a server restart
 * (vs. a live `didChangeConfiguration` update). Restart-triggering
 * settings are those that affect which binary runs or how the server
 * starts.
 */
export function checkIfConfigurationChanged(
    oldSettings: ISettings,
    newSettings: ISettings,
): boolean {
    return (
        JSON.stringify(oldSettings.path) !== JSON.stringify(newSettings.path) ||
        oldSettings.importStrategy !== newSettings.importStrategy ||
        oldSettings.configuration !== newSettings.configuration ||
        oldSettings.logLevel !== newSettings.logLevel
    );
}

/**
 * Resolve VS Code variables in path-shaped settings:
 * ${workspaceFolder}, ${userHome}, ${env:VAR}, etc.
 */
export function resolveVariables(
    value: string | undefined,
    folder?: vscode.WorkspaceFolder,
): string | undefined {
    if (!value) return undefined;
    let resolved = value;
    if (folder) {
        resolved = resolved.replace(
            /\$\{workspaceFolder\}/g,
            folder.uri.fsPath,
        );
    }
    resolved = resolved.replace(/\$\{userHome\}/g, process.env.HOME ?? process.env.USERPROFILE ?? "");
    resolved = resolved.replace(/\$\{cwd\}/g, process.cwd());
    resolved = resolved.replace(
        /\$\{env:(\w+)\}/g,
        (_, name) => process.env[name] ?? "",
    );
    return resolved;
}
