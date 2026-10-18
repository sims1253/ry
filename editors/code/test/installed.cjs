const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { createHash } = require("node:crypto");
const { execFileSync, spawn } = require("node:child_process");
const {
  downloadAndUnzipVSCode,
  resolveCliPathFromVSCodeExecutablePath,
} = require("@vscode/test-electron");

exports.run = () =>
  new Promise((resolve, reject) => {
    const mocha = new (require("mocha"))({ timeout: 30000 });
    mocha.addFile(path.resolve(__dirname, "../out/test/e2e.test.js"));
    mocha.addFile(path.resolve(__dirname, "../out/test/journey.test.js"));
    const results = {
      editorVersion: require("vscode").version,
      tests: [],
      failures: [],
    };
    const persist = () =>
      writeJson(path.join(process.env.RY_TEST_RESULTS, "tests.json"), results);
    persist();
    const runner = mocha.run((failures) => {
      persist();
      failures ? reject(new Error(`${failures} tests failed`)) : resolve();
    });
    runner.on("test end", (test) => {
      results.tests.push({
        title: test.fullTitle(),
        status: test.state,
        duration: test.duration,
      });
      persist();
    });
    runner.on("fail", (test, error) => {
      results.failures.push({ title: test.fullTitle(), error: error.stack });
      persist();
    });
  });

function writeJson(file, value) {
  fs.writeFileSync(file, JSON.stringify(value, null, 2) + "\n");
}

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const shellQuote = (value) => "'" + value.replace(/'/g, "'\\''") + "'";

function writeRerun(output, editorVersion) {
  fs.writeFileSync(
    path.join(output, "rerun.sh"),
    `#!/usr/bin/env bash
set -euo pipefail
artifact_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
rerun_tmp="$(mktemp -d)"
trap 'rm -rf -- "$rerun_tmp"' EXIT
cp "$artifact_dir/ry.vsix" "$rerun_tmp/ry.vsix"
cd "\${1:-$PWD}/editors/code"
export RY_TEST_VSIX="$rerun_tmp/ry.vsix"
export RY_TEST_VSCODE_VERSION=${shellQuote(editorVersion)}
if [ -n "\${DISPLAY:-}" ]; then
  npm test
else
  xvfb-run -a npm test
fi
`,
  );
}

async function main() {
  const output = path.resolve(__dirname, "../test-results/installed");
  const vsix = fs.readFileSync(
    process.env.RY_TEST_VSIX || path.resolve(__dirname, "../ry.vsix"),
  );
  fs.rmSync(output, { recursive: true, force: true });
  fs.mkdirSync(output, { recursive: true });
  fs.writeFileSync(path.join(output, "ry.vsix"), vsix);
  const manifest = {
    schemaVersion: 1,
    status: "running",
    commit: execFileSync("git", ["rev-parse", "HEAD"], {
      encoding: "utf8",
    }).trim(),
    workingTree: execFileSync("git", ["status", "--short"], {
      encoding: "utf8",
    }).trim(),
    nodeVersion: process.version,
    vsixSha256: hash(vsix),
    requestedEditorVersion: process.env.RY_TEST_VSCODE_VERSION || "stable",
  };
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "ry-installed-"));
  const manifestPath = path.join(output, "manifest.json");
  writeJson(manifestPath, manifest);
  writeRerun(output, manifest.requestedEditorVersion);
  try {
    manifest.bundledBinarySha256 = hash(
      execFileSync(
        "unzip",
        ["-p", path.join(output, "ry.vsix"), "extension/bundled/bin/ry"],
        { maxBuffer: 256 * 1024 * 1024 },
      ),
    );
    const executable = await downloadAndUnzipVSCode(
      manifest.requestedEditorVersion,
    );
    manifest.editorVersion = JSON.parse(
      fs.readFileSync(
        path.join(path.dirname(executable), "resources/app/package.json"),
        "utf8",
      ),
    ).version;
    writeJson(manifestPath, manifest);
    writeRerun(output, manifest.editorVersion);
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
        path.join(output, "ry.vsix"),
      ],
      { stdio: "inherit" },
    );
    for (const trusted of [true, false]) {
      const evidence = path.join(output, trusted ? "trusted" : "untrusted");
      fs.mkdirSync(evidence);
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
          "ry.logLevel": "debug",
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
      try {
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
              stdio: ["ignore", "pipe", "pipe"],
              env: {
                ...process.env,
                RY_TEST_TRUSTED: String(trusted),
                RY_TEST_RESULTS: evidence,
              },
            },
          );
          child.stdout.on("data", (data) => {
            process.stdout.write(data);
            fs.appendFileSync(path.join(evidence, "stdout.log"), data);
          });
          child.stderr.on("data", (data) => {
            process.stderr.write(data);
            fs.appendFileSync(path.join(evidence, "stderr.log"), data);
          });
          child.on("error", reject);
          child.on("close", (code) =>
            code === 0
              ? resolve()
              : reject(new Error(`VS Code exited with ${code}`)),
          );
        });
      } finally {
        fs.cpSync(workspace, path.join(evidence, "workspace"), {
          recursive: true,
        });
        const logs = path.join(profile, "logs");
        if (fs.existsSync(logs)) {
          fs.cpSync(logs, path.join(evidence, "logs"), { recursive: true });
        }
      }
    }
    manifest.status = "passed";
  } catch (error) {
    manifest.status = "failed";
    manifest.error = error.stack;
    throw error;
  } finally {
    writeJson(manifestPath, manifest);
    fs.rmSync(root, { recursive: true, force: true });
  }
}

if (require.main === module)
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
