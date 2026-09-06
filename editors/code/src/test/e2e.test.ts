import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { expect } from "chai";

async function waitFor(check: () => boolean, message: string): Promise<void> {
  const deadline = Date.now() + 15000;
  while (!check() && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  expect(check(), message).to.equal(true);
}

async function debugInformation(): Promise<string> {
  await vscode.commands.executeCommand("ry.debugInformation");
  return vscode.window.activeTextEditor!.document.getText();
}

const shellQuote = (value: string) => "'" + value.replace(/'/g, "'\\''") + "'";

describe("Installed ry extension", () => {
  it("uses the packaged binary and safely replaces a running server", async function () {
    this.timeout(60000);
    const trusted = process.env.RY_TEST_TRUSTED === "true";
    expect(vscode.workspace.isTrusted).to.equal(trusted);
    const extension = vscode.extensions.getExtension("sims1253.ry")!;
    expect(extension.extensionPath).to.include(
      `${path.sep}extensions${path.sep}`,
    );
    const root = vscode.workspace.workspaceFolders![0].uri.fsPath;
    const uri = vscode.Uri.file(path.join(root, "bad.R"));
    const doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc);
    const hasArithmeticError = (line: number) =>
      vscode.languages
        .getDiagnostics(uri)
        .some((d) => String(d.code) === "RY040" && d.range.start.line === line);
    await waitFor(() => hasArithmeticError(0), "packaged server diagnostics");
    const binary = path.join(extension.extensionPath, "bundled", "bin", "ry");
    expect(await debugInformation()).to.include(binary);
    expect(fs.existsSync(path.join(root, "decoy.ran"))).to.equal(false);
    if (!trusted) return;

    const config = vscode.workspace.getConfiguration("ry", uri);
    const replacement = path.join(root, "replacement");
    // exec preserves this PID, so the marker identifies the observed server.
    fs.writeFileSync(
      replacement,
      `#!/bin/sh\nif [ "$1" = server ]; then echo $$ > "$0.pid"; fi\nexec ${shellQuote(binary)} "$@"\n`,
      { mode: 0o755 },
    );
    await config.update(
      "path",
      [replacement],
      vscode.ConfigurationTarget.Workspace,
    );
    await vscode.commands.executeCommand("ry.restart");
    expect(await debugInformation()).to.include(replacement);
    const pid = Number(fs.readFileSync(`${replacement}.pid`, "utf8").trim());
    process.kill(pid, 0);

    for (const failure of ["probe", "startup"]) {
      const invalid = path.join(root, `invalid-${failure}`);
      fs.writeFileSync(
        invalid,
        failure === "probe"
          ? "#!/bin/sh\necho invalid-version\n"
          : `#!/bin/sh\nif [ "$1" = version ]; then exec ${shellQuote(binary)} "$@"; fi\nexit 1\n`,
        { mode: 0o755 },
      );
      await config.update(
        "path",
        [invalid],
        vscode.ConfigurationTarget.Workspace,
      );
      await vscode.commands.executeCommand("ry.restart");
      expect(await debugInformation()).to.include(replacement);
      process.kill(pid, 0);
      const edit = new vscode.WorkspaceEdit();
      edit.insert(uri, new vscode.Position(0, 0), "# still served\n");
      await vscode.workspace.applyEdit(edit);
      await waitFor(
        () => hasArithmeticError(failure === "probe" ? 1 : 2),
        "old server continues checking edits",
      );
    }
    // Recover from a failed setting without reloading the extension host.
    await config.update("path", [], vscode.ConfigurationTarget.Workspace);
    await vscode.commands.executeCommand("ry.restart");
    expect(await debugInformation()).to.include(binary);
    expect(() => process.kill(pid, 0)).to.throw();
    expect(doc.getText()).to.include("still served");
  });
});
