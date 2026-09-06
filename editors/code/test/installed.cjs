const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync, spawn } = require("node:child_process");
const {
  downloadAndUnzipVSCode,
  resolveCliPathFromVSCodeExecutablePath,
} = require("@vscode/test-electron");

exports.run = () =>
  new Promise((resolve, reject) => {
    const mocha = new (require("mocha"))({ timeout: 30000 });
    mocha.addFile(path.resolve(__dirname, "../out/test/e2e.test.js"));
    mocha.run((failures) =>
      failures ? reject(new Error(`${failures} tests failed`)) : resolve(),
    );
  });

async function main() {
  const executable = await downloadAndUnzipVSCode("stable");
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "ry-installed-"));
  try {
    const driver = path.join(root, "driver");
    fs.mkdirSync(driver);
    fs.writeFileSync(
      path.join(driver, "package.json"),
      JSON.stringify({
        name: "ry-test-driver",
        version: "0.0.0",
        publisher: "test",
        engines: { vscode: "^1.90.0" },
        capabilities: { untrustedWorkspaces: { supported: true } },
      }),
    );
    const extensionDir = path.join(root, "extensions");
    const cli = resolveCliPathFromVSCodeExecutablePath(executable);
    execFileSync(
      cli,
      [
        "--extensions-dir",
        extensionDir,
        "--user-data-dir",
        path.join(root, "installer"),
        "--install-extension",
        path.resolve(__dirname, "../ry.vsix"),
      ],
      { stdio: "inherit" },
    );
    for (const trusted of [true, false]) {
      const workspace = path.join(root, trusted ? "trusted" : "untrusted");
      const profile = path.join(
        root,
        trusted ? "trusted-profile" : "untrusted-profile",
      );
      fs.mkdirSync(path.join(workspace, ".vscode"), { recursive: true });
      fs.mkdirSync(path.join(profile, "User"), { recursive: true });
      fs.writeFileSync(
        path.join(profile, "User/settings.json"),
        JSON.stringify({
          "security.workspace.trust.enabled": !trusted,
          "security.workspace.trust.startupPrompt": "never",
        }),
      );
      fs.writeFileSync(
        path.join(workspace, ".vscode/settings.json"),
        JSON.stringify({
          "ry.path": trusted ? [] : [path.join(workspace, "decoy")],
          "ry.importStrategy": "useBundled",
        }),
      );
      fs.writeFileSync(
        path.join(workspace, "decoy"),
        '#!/bin/sh\necho executed > "$0.ran"\nexit 1\n',
        { mode: 0o755 },
      );
      fs.writeFileSync(path.join(workspace, "bad.R"), 'x <- 1 + "a"\n');
      // test-electron's runTests always disables workspace trust.
      await new Promise((resolve, reject) => {
        const child = spawn(
          executable,
          [
            workspace,
            "--extensions-dir",
            extensionDir,
            "--user-data-dir",
            profile,
            "--skip-welcome",
            "--skip-release-notes",
            "--no-sandbox",
            "--disable-gpu-sandbox",
            "--disable-updates",
            `--extensionDevelopmentPath=${driver}`,
            `--extensionTestsPath=${__filename}`,
          ],
          {
            stdio: "inherit",
            env: { ...process.env, RY_TEST_TRUSTED: String(trusted) },
          },
        );
        child.on("error", reject);
        child.on("exit", (code) =>
          code === 0
            ? resolve()
            : reject(new Error(`VS Code exited with ${code}`)),
        );
      });
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

if (require.main === module)
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
