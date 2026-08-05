/**
 * End-to-end test: activate on .R, reach Running, produce diagnostics
 * from a known-bad fixture.
 */

import * as path from "path";
import * as vscode from "vscode";

import { expect } from "chai";

describe("E2E: ry extension", () => {
    it("Activates on .R file and produces diagnostics", async function () {
        this.timeout(30000);

        // Open the known-bad fixture
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

        // Wait for diagnostics
        await new Promise((resolve) => setTimeout(resolve, 2000));

        const diagnostics = vscode.languages.getDiagnostics(uri);
        expect(diagnostics.length).to.be.greaterThan(0);
    });
});
