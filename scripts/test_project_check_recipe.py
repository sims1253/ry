"""Exercise the published project-check hook and CI command with a real ry binary."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXAMPLE = ROOT / "docs/examples/project-check"


def run(
    argv: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    expected: int = 0,
    contains: str | None = None,
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        argv, cwd=cwd, env=env, text=True, capture_output=True, check=False
    )
    output = result.stdout + result.stderr
    if result.returncode != expected or (contains and contains not in output):
        raise AssertionError(
            f"{argv!r}: exit {result.returncode}, expected {expected}; "
            f"required text {contains!r}; output:\n{output}"
        )
    return result


def read_recipe() -> str:
    import yaml

    hook_config = yaml.safe_load((EXAMPLE / ".pre-commit-config.yaml").read_text())
    [hook] = hook_config["repos"][0]["hooks"]
    assert hook_config["repos"][0]["repo"] == "local"
    assert hook["entry"] == "ry check ."
    assert hook["language"] == "system"
    assert hook["pass_filenames"] is False
    assert hook["always_run"] is True
    assert hook["stages"] == ["pre-commit"]

    workflow = yaml.safe_load((EXAMPLE / ".github/workflows/ry.yml").read_text())
    steps = workflow["jobs"]["check"]["steps"]
    install = next(step for step in steps if step.get("name") == "Install ry 0.11.0")
    assert (
        "https://github.com/sims1253/ry/releases/download/v0.11.0/ry-cli-installer.sh"
    ) in install["run"]
    assert install["env"]["RY_CLI_NO_MODIFY_PATH"] == "1"
    check = next(step for step in steps if step.get("name") == "Check project")
    commands = check["run"].splitlines()
    assert 'test "$(ry --version)" = "ry 0.11.0"' in commands
    assert "ry check --output-format github ." in commands
    assert "--exit-zero" not in check["run"]
    return check["run"]


def git(project: Path, env: dict[str, str], *args: str) -> None:
    run(["git", *args], cwd=project, env=env)


def assert_finding(
    result: subprocess.CompletedProcess[str], code: str, path: str
) -> None:
    output = result.stdout + result.stderr
    assert code in output and path in output, output


def assert_hook_invokes_ry(
    runner: Path, project: Path, env: dict[str, str], call_log: Path
) -> None:
    before = call_log.read_text()
    run(
        [str(runner), "run", "ry-project", "--hook-stage", "pre-commit"],
        cwd=project,
        env=env,
    )
    after = call_log.read_text()
    assert after.count("check .\n") == before.count("check .\n") + 1, after


def check_runner(
    runner: Path,
    project: Path,
    env: dict[str, str],
    ci_script: str,
    call_log: Path,
) -> None:
    # A successful source tree passes direct invocation, the hook, and the
    # actual command copied from the committed workflow.
    run(["ry", "check", "."], cwd=project, env=env)
    assert_hook_invokes_ry(runner, project, env, call_log)
    run(["bash", "-e", "-o", "pipefail", "-c", ci_script], cwd=project, env=env)

    # Only the callee is staged. The unchanged caller must still fail in all
    # three modes, which a changed-filename-only hook would miss.
    helper = project / "R/helper.R"
    helper.write_text('value <- function() "text"\n')
    git(project, env, "add", "R/helper.R")
    direct = run(["ry", "check", "."], cwd=project, env=env, expected=1)
    hook = run(
        [str(runner), "run", "ry-project", "--hook-stage", "pre-commit"],
        cwd=project,
        env=env,
        expected=1,
    )
    ci = run(
        ["bash", "-e", "-o", "pipefail", "-c", ci_script],
        cwd=project,
        env=env,
        expected=1,
    )
    for result in (direct, hook, ci):
        assert_finding(result, "RY040", "R/caller.R")
    git(project, env, "restore", "--staged", "R/helper.R")
    git(project, env, "restore", "R/helper.R")

    # Each non-R input is the only staged change. The hook must invoke ry
    # even though no R filename was passed to it.
    for name in [
        "ry.toml",
        "ry-baseline.json",
        "stubs/pkg.json",
        "DESCRIPTION",
        "NAMESPACE",
    ]:
        path = project / name
        original = path.read_text()
        path.write_text(original + "\n")
        git(project, env, "add", name)
        assert_hook_invokes_ry(runner, project, env, call_log)
        git(project, env, "restore", "--staged", name)
        git(project, env, "restore", name)

    # The example's warning policy fails on a new warning. A reviewed
    # baseline accepts that identity but does not swallow a new one.
    warning = project / "R/warn.R"
    warning.write_text("first_missing\n")
    git(project, env, "add", "R/warn.R")
    run(["ry", "check", "."], cwd=project, env=env, expected=1, contains="RY010")
    run(
        ["ry", "check", "--write-baseline", "ry-baseline.json", "."],
        cwd=project,
        env=env,
        expected=1,
        contains="RY010",
    )
    config = project / "ry.toml"
    config.write_text(config.read_text().replace("# baseline =", "baseline ="))
    git(project, env, "add", "ry.toml", "ry-baseline.json")
    run(["ry", "check", "."], cwd=project, env=env)
    assert_hook_invokes_ry(runner, project, env, call_log)
    run(["bash", "-e", "-o", "pipefail", "-c", ci_script], cwd=project, env=env)
    warning.write_text("first_missing\nsecond_missing\n")
    git(project, env, "add", "R/warn.R")
    direct = run(["ry", "check", "."], cwd=project, env=env, expected=1)
    hook = run(
        [str(runner), "run", "ry-project", "--hook-stage", "pre-commit"],
        cwd=project,
        env=env,
        expected=1,
    )
    ci = run(
        ["bash", "-e", "-o", "pipefail", "-c", ci_script],
        cwd=project,
        env=env,
        expected=1,
    )
    for result in (direct, hook, ci):
        assert_finding(result, "RY010", "R/warn.R")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ry", type=Path, required=True)
    parser.add_argument("--pre-commit", type=Path, required=True)
    parser.add_argument("--prek", type=Path, required=True)
    args = parser.parse_args()
    binary = args.ry.resolve(strict=True)
    ci_script = read_recipe()
    for runner in [
        args.pre_commit.resolve(strict=True),
        args.prek.resolve(strict=True),
    ]:
        with tempfile.TemporaryDirectory(prefix="ry-project-recipe-") as temporary:
            project = Path(temporary)
            shutil.copyfile(
                EXAMPLE / ".pre-commit-config.yaml",
                project / ".pre-commit-config.yaml",
            )
            shutil.copyfile(EXAMPLE / "ry.toml", project / "ry.toml")
            (project / "R").mkdir()
            (project / "stubs").mkdir()
            (project / "R/helper.R").write_text("value <- function() 1L\n")
            (project / "R/caller.R").write_text("result <- value() + 1L\n")
            (project / "DESCRIPTION").write_text("Package: recipe\nVersion: 0.1\n")
            (project / "NAMESPACE").write_text("# no imports\n")
            (project / "ry-baseline.json").write_text("{}\n")
            (project / "stubs/pkg.json").write_text("{}\n")
            shim_dir = project / "shim"
            shim_dir.mkdir()
            call_log = project / "calls.txt"
            call_log.write_text("")
            shim = shim_dir / "ry"
            shim.write_text(
                "#!/bin/sh\n"
                'printf "%s\\n" "$*" >> "$RY_RECIPE_CALL_LOG"\n'
                'exec "$RY_RECIPE_BINARY" "$@"\n'
            )
            shim.chmod(0o755)
            env = os.environ.copy()
            env.update(
                {
                    "RY_RECIPE_BINARY": str(binary),
                    "RY_RECIPE_CALL_LOG": str(call_log),
                    "PATH": f"{shim_dir}{os.pathsep}{env['PATH']}",
                    "PRE_COMMIT_HOME": str(project / "pre-commit-cache"),
                    "PREK_HOME": str(project / "prek-cache"),
                }
            )
            git(project, env, "init", "-q")
            git(project, env, "config", "user.name", "ry recipe fixture")
            git(project, env, "config", "user.email", "recipe@example.invalid")
            git(
                project,
                env,
                "add",
                ".pre-commit-config.yaml",
                "ry.toml",
                "R",
                "DESCRIPTION",
                "NAMESPACE",
                "ry-baseline.json",
                "stubs",
            )
            git(project, env, "commit", "-qm", "fixture: clean project")
            check_runner(runner, project, env, ci_script, call_log)
            print(
                f"PASS: {runner.name} project context, metadata triggers, and baseline"
            )


if __name__ == "__main__":
    main()
