/**
 * End-to-end test: activate on .R, reach Running, produce diagnostics
 * from a known-bad fixture.
 */

import * as path from "path";
import * as vscode from "vscode";

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
        const suppressedCode = "RY010";
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
        // The fixture has RY040 (invalid arithmetic) and RY010 (unbound
        // variable). The ry.toml ignores RY010, so only RY040 should appear.
        expect(codes).to.include(expectedCode);
        expect(codes).to.not.include(suppressedCode);
    });
});
