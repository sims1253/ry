/**
 * Command implementations — `ry.debugInformation`, `ry.explainRule`.
 */

import * as vscode from "vscode";
import { errorMessage } from "./errors";
import { runBinary } from "./process";
import { ResolvedBinary } from "./binary";
import { type ISettings } from "./settings";

export async function debugInformationCommand(
  binary: ResolvedBinary | undefined,
  settings: ISettings | undefined,
) {
  const lines: string[] = [];
  lines.push("## ry debug information");
  lines.push("");
  lines.push(`**Binary path**: ${binary?.path ?? "not resolved"}`);
  lines.push(
    `**Version**: ${
      binary?.version
        ? `${binary.version.major}.${binary.version.minor}.${binary.version.patch}`
        : "unknown"
    }`,
  );
  lines.push("");
  lines.push("**Settings:**");
  lines.push("```json");
  lines.push(JSON.stringify(settings ?? {}, null, 2));
  lines.push("```");
  lines.push("");
  lines.push("**Workspace folders:**");
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    lines.push(`- ${folder.name}: ${folder.uri.fsPath}`);
  }

  const output = lines.join("\n");
  const doc = await vscode.workspace.openTextDocument({
    content: output,
    language: "markdown",
  });
  await vscode.window.showTextDocument(doc);
}

export async function explainRuleCommand(binaryPath: string): Promise<void> {
  try {
    const { stdout } = await runBinary(binaryPath, [
      "explain",
      "rule",
      "--output-format",
      "json",
    ]);
    const rules: unknown = JSON.parse(stdout);
    if (
      !Array.isArray(rules) ||
      !rules.every(
        (rule): rule is { code: string; name: string; summary: string } =>
          rule &&
          typeof rule === "object" &&
          typeof rule.code === "string" &&
          typeof rule.name === "string" &&
          typeof rule.summary === "string",
      )
    )
      throw new Error("Invalid rule list from ry");
    const items = rules.map((rule) => ({
      label: rule.code,
      description: rule.name,
      detail: rule.summary,
    }));
    const picked = await vscode.window.showQuickPick(items, {
      placeHolder: "Select a rule to explain",
    });
    if (!picked) return;
    const doc = await vscode.workspace.openTextDocument({
      content: `# ${picked.label}: ${picked.description}\n\n${picked.detail}`,
      language: "markdown",
    });
    await vscode.window.showTextDocument(doc, { preview: true });
  } catch (error) {
    await vscode.window.showErrorMessage(
      `Failed to explain rule: ${errorMessage(error)}`,
    );
  }
}
