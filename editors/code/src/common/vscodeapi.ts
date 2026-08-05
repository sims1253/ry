// Thin wrappers around the `vscode` API so the rest of the client is
// unit-testable without a live VS Code host. Mirrors the module of the
// same name in ruff-vscode and ty-vscode.

import {
  commands,
  type ConfigurationScope,
  type Disposable,
  workspace,
  type WorkspaceConfiguration,
  type WorkspaceFolder,
} from "vscode";

export function getConfiguration(
  config: string,
  scope?: ConfigurationScope,
): WorkspaceConfiguration {
  return workspace.getConfiguration(config, scope);
}

export function registerCommand(
  command: string,
  callback: (...args: unknown[]) => unknown,
  thisArg?: unknown,
): Disposable {
  return commands.registerCommand(command, callback, thisArg);
}

export const { onDidChangeConfiguration, onDidGrantWorkspaceTrust } = workspace;

export function isVirtualWorkspace(): boolean {
  const isVirtual =
    workspace.workspaceFolders !== undefined &&
    workspace.workspaceFolders.every((f) => f.uri.scheme !== "file");
  return isVirtual;
}

export function getWorkspaceFolders(): readonly WorkspaceFolder[] {
  return workspace.workspaceFolders ?? [];
}
