import { execFile } from "child_process";
import { promisify } from "util";

const execFileAsync = promisify(execFile);

export function runBinary(
  binaryPath: string,
  args: string[],
  signal?: AbortSignal,
) {
  return execFileAsync(binaryPath, args, {
    encoding: "utf-8",
    timeout: 5000,
    signal,
  });
}
