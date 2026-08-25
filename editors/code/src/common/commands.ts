/**
 * Command implementations — `ry.showLogs`, `ry.showServerLogs`,
 * `ry.debugInformation`, `ry.explainRule`.
 */

import * as vscode from "vscode";
import * as cp from "child_process";
import { Logger } from "./logger";
import { ResolvedBinary } from "./binary";
import { type ISettings } from "./settings";

export function showLogsCommand(logger: Logger): void {
  logger.channel.show();
}

export function showServerLogsCommand(
  serverChannel: vscode.OutputChannel,
): void {
  serverChannel.show();
}

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
  // Show a quick-pick over all rules, then render the explanation.
  // `ry explain rule <code> --output-format json` already exists.
  try {
    const listOutput = cp.execSync(
      `"${binaryPath}" explain rules --output-format json`,
      {
        encoding: "utf-8",
        timeout: 5000,
      },
    );
    const rules = JSON.parse(listOutput) as Array<{
      code: string;
      name: string;
    }>;
    const items = rules.map((r) => ({
      label: r.code,
      description: r.name,
    }));
    const picked = await vscode.window.showQuickPick(items, {
      placeHolder: "Select a rule to explain",
    });
    if (!picked) return;

    const explainOutput = cp.execSync(
      `"${binaryPath}" explain rule ${picked.label} --output-format json`,
      { encoding: "utf-8", timeout: 5000 },
    );
    const explanation = JSON.parse(explainOutput);
    const md = `# ${explanation.code}: ${explanation.name}\n\n${explanation.explanation ?? ""}`;
    const doc = vscode.workspace.openTextDocument({
      content: md,
      language: "markdown",
    });
    doc.then((d) => vscode.window.showTextDocument(d, { preview: true }));
  } catch (e) {
    vscode.window.showErrorMessage(`Failed to explain rule: ${e}`);
  }
}
