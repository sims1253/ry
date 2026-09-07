import { execFile } from "child_process";
import { promisify } from "util";
import { Data, Effect } from "effect";

const execFileAsync = promisify(execFile);

export class ProcessError extends Data.TaggedError("ProcessError")<{
  readonly binaryPath: string;
  readonly cause: unknown;
}> {}

/** Cancellation interrupts the child process; the timeout bounds CLI probes. */
export const runBinary = (binaryPath: string, args: string[]) =>
  Effect.tryPromise({
    try: (signal) =>
      execFileAsync(binaryPath, args, {
        encoding: "utf-8",
        timeout: 5000,
        signal,
      }),
    catch: (cause) => new ProcessError({ binaryPath, cause }),
  });
