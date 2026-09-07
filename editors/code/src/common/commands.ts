/**
 * Command implementations — `ry.debugInformation`, `ry.explainRule`.
 */

import * as vscode from "vscode";
import { Effect, Schema } from "effect";
import { errorMessage } from "./errors";
import { runBinary } from "./process";
import { ResolvedBinary } from "./binary";
import { type ISettings } from "./settings";

export const debugInformationCommand = (
  binary: ResolvedBinary | undefined,
  settings: ISettings | undefined,
) =>
  Effect.gen(function* () {
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
    const doc = yield* Effect.tryPromise(() =>
      Promise.resolve(
        vscode.workspace.openTextDocument({
          content: output,
          language: "markdown",
        }),
      ),
    );
    yield* Effect.tryPromise(() =>
      Promise.resolve(vscode.window.showTextDocument(doc)),
    );
  });

const decodeRules = Schema.decodeUnknown(
  Schema.parseJson(
    Schema.Array(
      Schema.Struct({
        code: Schema.String,
        name: Schema.String,
        summary: Schema.String,
      }),
    ),
  ),
);

export const explainRuleCommand = (binaryPath: string) =>
  Effect.gen(function* () {
    const { stdout } = yield* runBinary(binaryPath, [
      "explain",
      "rule",
      "--output-format",
      "json",
    ]);
    const rules = yield* decodeRules(stdout);
    const items = rules.map((rule) => ({
      label: rule.code,
      description: rule.name,
      detail: rule.summary,
    }));
    const picked = yield* Effect.tryPromise(() =>
      Promise.resolve(
        vscode.window.showQuickPick(items, {
          placeHolder: "Select a rule to explain",
        }),
      ),
    );
    if (!picked) return;
    const doc = yield* Effect.tryPromise(() =>
      Promise.resolve(
        vscode.workspace.openTextDocument({
          content: `# ${picked.label}: ${picked.description}\n\n${picked.detail}`,
          language: "markdown",
        }),
      ),
    );
    yield* Effect.tryPromise(() =>
      Promise.resolve(vscode.window.showTextDocument(doc, { preview: true })),
    );
  }).pipe(
    Effect.catchAll((error) =>
      Effect.tryPromise(() =>
        Promise.resolve(
          vscode.window.showErrorMessage(
            `Failed to explain rule: ${errorMessage(error)}`,
          ),
        ),
      ),
    ),
    Effect.asVoid,
  );
