const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { performance } = require("node:perf_hooks");

// Invoked inside a fresh extension host by @vscode/test-electron.
exports.run = async () => {
  const vscode = require("vscode");
  const extension = vscode.extensions.getExtension("scholzmx.ry");
  assert.ok(extension, "ry extension is available");
  assert.equal(extension.isActive, false, "ry must not activate before timing");
  assert.equal(vscode.workspace.isTrusted, true);

  const start = performance.now();
  await extension.activate();
  const activation = performance.now() - start;

  // activate() schedules server startup, so also measure useful readiness.
  // A real diagnostic ensures a failed server cannot look like a speedup.
  const uri = vscode.Uri.joinPath(
    vscode.workspace.workspaceFolders[0].uri,
    "activation.R",
  );
  let subscription;
  let timer;
  const diagnostic = new Promise((resolve, reject) => {
    subscription = vscode.languages.onDidChangeDiagnostics(() => {
      if (
        vscode.languages
          .getDiagnostics(uri)
          .some((item) => String(item.code) === "RY040")
      ) {
        resolve(performance.now() - start);
      }
    });
    timer = setTimeout(
      () => reject(new Error("No RY040 diagnostic within 30 seconds")),
      30000,
    );
  });
  // Attach the rejection handler before opening the document.
  const ready = (async () => {
    try {
      await vscode.workspace.fs.writeFile(uri, Buffer.from('x <- 1 + "a"\n'));
      const document = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(document);
    } catch (error) {
      clearTimeout(timer);
      subscription.dispose();
      throw error;
    }
  })();
  try {
    const [firstDiagnostic] = await Promise.all([diagnostic, ready]);
    fs.writeFileSync(
      process.env.RY_PERFORMANCE_SAMPLE,
      JSON.stringify({ activation, firstDiagnostic, vscode: vscode.version }),
    );
  } finally {
    clearTimeout(timer);
    subscription.dispose();
  }
};

async function main() {
  const { downloadAndUnzipVSCode, runTests } = require("@vscode/test-electron");
  // Fix the host version so editor upgrades do not silently shift the baseline.
  const version = "1.90.2";
  const binary = path.resolve(__dirname, "../../../target/release/ry");
  assert.ok(fs.existsSync(binary), "build the release ry binary first");
  const destination = process.argv[2];
  assert.ok(destination, "pass an output JSON path");
  const executable = await downloadAndUnzipVSCode(version);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "ry-performance-"));
  const samples = [];
  try {
    for (let index = 0; index < 5; index++) {
      const directory = path.join(root, String(index));
      const workspace = path.join(directory, "workspace");
      const output = path.join(directory, "sample.json");
      fs.mkdirSync(path.join(workspace, ".vscode"), { recursive: true });
      fs.writeFileSync(
        path.join(workspace, ".vscode/settings.json"),
        JSON.stringify({ "ry.path": [binary] }),
      );
      await runTests({
        vscodeExecutablePath: executable,
        extensionDevelopmentPath: path.resolve(__dirname, ".."),
        extensionTestsPath: __filename,
        extensionTestsEnv: { RY_PERFORMANCE_SAMPLE: output },
        launchArgs: [
          workspace,
          "--user-data-dir",
          path.join(directory, "profile"),
          "--extensions-dir",
          path.join(directory, "extensions"),
          "--skip-welcome",
          "--skip-release-notes",
          "--no-sandbox",
          "--disable-gpu",
          "--disable-updates",
        ],
      });
      samples.push(JSON.parse(fs.readFileSync(output, "utf8")));
    }
    const results = [
      ["vscode/activation", "activation"],
      ["vscode/activation-to-first-diagnostic", "firstDiagnostic"],
    ].map(([name, key]) => {
      const values = samples.map((sample) => sample[key]).sort((a, b) => a - b);
      assert.ok(values.every((value) => Number.isFinite(value) && value > 0));
      return {
        name,
        unit: "ms",
        value: values[2],
        range: `${values[0].toFixed(2)}–${values[4].toFixed(2)}`,
        extra: `VS Code ${version}; median of 5 fresh hosts; samples: ${values.join(", ")}`,
      };
    });
    fs.writeFileSync(destination, JSON.stringify(results, null, 2) + "\n");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

if (require.main === module)
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
