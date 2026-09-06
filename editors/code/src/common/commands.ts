/**
 * Command implementations — `ry.debugInformation`, `ry.explainRule`.
 */

import * as vscode from "vscode";
import { execFile } from "child_process";
import { promisify } from "util";

const execFileAsync = promisify(execFile);
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
    const { stdout } = await execFileAsync(
      binaryPath,
      ["explain", "rule", "--output-format", "json"],
      {
        encoding: "utf-8",
        timeout: 5000,
      },
    );
    const rules: unknown = JSON.parse(stdout);
    if (!Array.isArray(rules)) throw new Error("Expected a rule list");
    const items = rules.map((rule: unknown) => {
      if (
        typeof rule !== "object" ||
        rule === null ||
        !("code" in rule) ||
        typeof rule.code !== "string" ||
        !("name" in rule) ||
        typeof rule.name !== "string" ||
        !("summary" in rule) ||
        typeof rule.summary !== "string"
      ) {
        throw new Error("Invalid rule description");
      }
      return { label: rule.code, description: rule.name, detail: rule.summary };
    });
    const picked = await vscode.window.showQuickPick(items, {
      placeHolder: "Select a rule to explain",
    });
    if (!picked) return;

    const md = `# ${picked.label}: ${picked.description}\n\n${picked.detail}`;
    const doc = await vscode.workspace.openTextDocument({
      content: md,
      language: "markdown",
    });
    await vscode.window.showTextDocument(doc, { preview: true });
  } catch (e) {
    vscode.window.showErrorMessage(`Failed to explain rule: ${e}`);
  }
}
