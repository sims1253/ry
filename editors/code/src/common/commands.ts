/**
 * Command implementations — `ry.debugInformation`, `ry.explainRule`.
 */

import * as vscode from "vscode";
import * as cp from "child_process";
import { ResolvedBinary } from "./binary";
import { type ISettings } from "./settings";

export async function debugInformationCommand(
  binary: ResolvedBinary | undefined,
  settings: ISettings | undefined,
): Promise<void> {
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
    const listOutput = cp.execFileSync(
      binaryPath,
      ["explain", "rule", "--output-format", "json"],
      {
        encoding: "utf-8",
        timeout: 5000,
      },
    );
    const rules = JSON.parse(listOutput) as Array<{
      code: string;
      name: string;
      summary: string;
    }>;
    const items = rules.map((r) => ({
      label: r.code,
      description: r.name,
      summary: r.summary,
    }));
    const picked = await vscode.window.showQuickPick(items, {
      placeHolder: "Select a rule to explain",
    });
    if (!picked) return;

    const md = `# ${picked.label}: ${picked.description}\n\n${picked.summary}`;
    const doc = await vscode.workspace.openTextDocument({
      content: md,
      language: "markdown",
    });
    await vscode.window.showTextDocument(doc, { preview: true });
  } catch (e) {
    vscode.window.showErrorMessage(`Failed to explain rule: ${e}`);
  }
}
