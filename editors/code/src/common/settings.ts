/**
 * Settings module — mirrors ruff-vscode's `settings.ts`.
 *
 * Provides `ISettings`, `getWorkspaceSettings`, `getGlobalSettings`,
 * `getExtensionSettings` (returning the per-folder array that feeds
 * `initializationOptions`), and `checkIfConfigurationChanged`.
 */

import * as vscode from "vscode";
import { getConfiguration, getWorkspaceFolders } from "./vscodeapi";

export interface ISettings {
  enable?: boolean;
  path?: string[];
  configuration?: string;
  importStrategy: "fromEnvironment" | "useBundled";
  lint: ILintSettings;
  minConfidence?: "low" | "medium" | "high";
  baseline?: string;
  logLevel?: string;
}

export interface ILintSettings {
  select?: string[];
  extendSelect?: string[];
  ignore?: string[];
  error?: string[];
  warn?: string[];
}

function getExplicitValue<T>(
  config: vscode.WorkspaceConfiguration,
  key: string,
): T | undefined {
  const inspected = config.inspect<T>(key);
  if (!inspected) return undefined;
  // Preserve an explicitly configured empty array while omitting the schema
  // default. Sending the default `[]` would mean "select no rules" to the
  // server and would suppress diagnostics in an otherwise unconfigured
  // workspace.
  return (
    inspected.workspaceFolderLanguageValue ??
    inspected.workspaceLanguageValue ??
    inspected.globalLanguageValue ??
    inspected.workspaceFolderValue ??
    inspected.workspaceValue ??
    inspected.globalValue
  );
}

export function getWorkspaceSettings(
  namespace: string,
  folder: vscode.WorkspaceFolder,
): ISettings {
  return readSettings(getConfiguration(namespace, folder.uri));
}

export function getGlobalSettings(namespace: string): ISettings {
  return readSettings(getConfiguration(namespace));
}

function readSettings(config: vscode.WorkspaceConfiguration): ISettings {
  return {
    enable: config.get<boolean>("enable"),
    path: config.get<string[]>("path"),
    configuration: config.get<string>("configuration"),
    importStrategy: config.get<"fromEnvironment" | "useBundled">(
      "importStrategy",
      "fromEnvironment",
    ),
    lint: {
      select: getExplicitValue<string[]>(config, "lint.select"),
      extendSelect: getExplicitValue<string[]>(config, "lint.extendSelect"),
      ignore: getExplicitValue<string[]>(config, "lint.ignore"),
      error: getExplicitValue<string[]>(config, "lint.error"),
      warn: getExplicitValue<string[]>(config, "lint.warn"),
    },
    minConfidence: config.get<"low" | "medium" | "high">("minConfidence"),
    baseline: config.get<string>("baseline"),
    logLevel: config.get<string>("logLevel"),
  };
}

/**
 * Build the per-folder settings array sent as
 * `initializationOptions` at `initialize` time.
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
