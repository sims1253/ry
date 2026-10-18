import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { expect } from "chai";

const original = 'x <- 1 + "a"\ny <- 2 + "b"\n';
const fixed = 'x <- 1 + 1\ny <- 2 + "b"\n';

function diagnostics(uri: vscode.Uri) {
  return vscode.languages.getDiagnostics(uri).map((diagnostic) => ({
    code:
      typeof diagnostic.code === "object"
        ? String(diagnostic.code.value)
        : String(diagnostic.code),
    severity: diagnostic.severity,
    start: [diagnostic.range.start.line, diagnostic.range.start.character],
    end: [diagnostic.range.end.line, diagnostic.range.end.character],
    message: diagnostic.message,
  }));
}

function arithmeticError(line: number) {
  return {
    code: "RY040",
    severity: vscode.DiagnosticSeverity.Error,
    start: [line, 5],
    end: [line, 12],
  };
}

describe("Installed diagnostic journey", () => {
  let document: vscode.TextDocument | undefined;

  function save(name: string, value: unknown) {
    fs.writeFileSync(
      path.join(process.env.RY_TEST_RESULTS!, `${name}.json`),
      JSON.stringify(value, null, 2) + "\n",
    );
  }

  async function checkpoint(name: string, lines: number[]) {
    const expected = lines.map(arithmeticError);
    const observed = () =>
      diagnostics(document!.uri).map(({ message: _message, ...rest }) => rest);
    const deadline = Date.now() + 15000;
    while (
      JSON.stringify(observed()) !== JSON.stringify(expected) &&
      Date.now() < deadline
    ) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    save(name, {
      source: document!.getText(),
      version: document!.version,
      expected,
      observed: diagnostics(document!.uri),
    });
    expect(observed(), name).to.deep.equal(expected);
  }

  afterEach(() => {
    if (document) {
      save("journey-final", {
        source: document.getText(),
        observed: diagnostics(document.uri),
      });
    }
  });

  it("fixes, undoes, and suppresses a diagnostic across a server restart", async function () {
    this.timeout(90000);
    const root = vscode.workspace.workspaceFolders![0].uri.fsPath;
    const uri = vscode.Uri.file(path.join(root, "journey.R"));
    fs.writeFileSync(uri.fsPath, original);
    document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);
    await checkpoint("01-original", [0, 1]);

    const editor = vscode.window.activeTextEditor!;
    expect(
      await editor.edit((edit) =>
        edit.replace(new vscode.Range(0, 0, document!.lineCount, 0), fixed),
      ),
    ).to.equal(true);
    await checkpoint("02-fixed", [1]);

    await vscode.commands.executeCommand("undo");
    expect(document.getText()).to.equal(original);
    await checkpoint("03-undone", [0, 1]);

    const actions = await vscode.commands.executeCommand<vscode.CodeAction[]>(
      "vscode.executeCodeActionProvider",
      uri,
      new vscode.Range(0, 5, 0, 12),
      vscode.CodeActionKind.QuickFix.value,
    );
    save("04-actions", actions);
    const action = actions?.find(
      (candidate) => candidate.title === "Ignore RY040 on this line",
    );
    expect(action?.edit, "line suppression edit").not.to.equal(undefined);
    expect(await vscode.workspace.applyEdit(action!.edit!)).to.equal(true);
    expect(document.lineAt(0).text).to.include("# ry: ignore[RY040]");
    expect(document.lineAt(1).text).to.equal('y <- 2 + "b"');
    await checkpoint("05-suppressed", [1]);
    expect(await document.save()).to.equal(true);
    const saved = document.getText();

    await vscode.commands.executeCommand("workbench.action.closeActiveEditor");
    await vscode.commands.executeCommand("ry.restart");
    document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);
    expect(document.getText()).to.equal(saved);
    await checkpoint("06-reopened", [1]);
  });
});
