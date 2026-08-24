/**
 * End-to-end tests for the VS Code extension.
 */

import * as path from "path";
import * as vscode from "vscode";
import { expect } from "chai";

describe("E2E: ry extension", () => {
  it("Activates on .R file and produces expected diagnostics", async function () {
    this.timeout(30000);

    const fixturePath = path.join(
      __dirname,
      "..",
      "..",
      "testFixture",
      "bad.R",
    );
    const uri = vscode.Uri.file(fixturePath);
    const doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc);

    // Bounded polling: wait for diagnostics with a timeout rather
    // than a fixed sleep.
    const expectedCode = "RY040";
    let diagnostics: vscode.Diagnostic[] = [];
    const deadline = Date.now() + 15000;
    while (Date.now() < deadline) {
      diagnostics = vscode.languages.getDiagnostics(uri);
      const codes = diagnostics.map((d) => String(d.code));
      if (codes.includes(expectedCode)) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 200));
    }

    const codes = diagnostics.map((d) => String(d.code));
    // The fixture has RY040 (invalid arithmetic). The extension must
    // activate and produce at least this diagnostic.
    expect(codes).to.include(expectedCode);
  });

  // The server launches from the single pre-resolved binary path; the
  // unit tests in binary.test.ts verify trust-honoring resolution.
  it("Server starts from the resolved binary path", async function () {
    this.timeout(30000);

    const fixturePath = path.join(
      __dirname,
      "..",
      "..",
      "testFixture",
      "bad.R",
    );
    const uri = vscode.Uri.file(fixturePath);
    const doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc);

    const deadline = Date.now() + 15000;
    let serverStarted = false;
    while (Date.now() < deadline) {
      const diags = vscode.languages.getDiagnostics(uri);
      if (diags.length > 0) {
        serverStarted = true;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    // Server started means startServer received a valid binaryPath.
    expect(serverStarted).to.equal(true);
  });
});
