import * as util from "util";
import * as vscode from "vscode";

class ExtensionLogger {
  /**
   * The output channel used to log messages for the extension.
   */
  readonly channel = vscode.window.createOutputChannel("ry", { log: true });

  /**
   * Whether the extension is running in a CI environment.
   */
  private readonly isCI = process.env.CI === "true";

  /**
   * Logs messages to the console if the extension is running in a CI environment.
   */
  private logForCI(...messages: unknown[]): void {
    if (this.isCI) {
      // eslint-disable-next-line no-console
      console.log(...messages);
    }
  }

  error(...messages: unknown[]): void {
    this.logForCI(...messages);
    this.channel.error(util.format(...messages));
  }

  warn(...messages: unknown[]): void {
    this.logForCI(...messages);
    this.channel.warn(util.format(...messages));
  }

  info(...messages: unknown[]): void {
    this.logForCI(...messages);
    this.channel.info(util.format(...messages));
  }

  debug(...messages: unknown[]): void {
    this.logForCI(...messages);
    this.channel.debug(util.format(...messages));
  }

  trace(...messages: unknown[]): void {
    this.logForCI(...messages);
    this.channel.trace(util.format(...messages));
  }
}

/**
 * The structural interface satisfied by [`ExtensionLogger`].
 *
 * Consumers that want to log (binary resolution, status reporting, etc.)
 * depend on this type rather than the concrete class, so they can be
 * unit-tested with a stub.
 */
export type Logger = Pick<
  ExtensionLogger,
  "channel" | "error" | "warn" | "info" | "debug" | "trace"
>;

/**
 * The logger used by the extension.
 *
 * This logs messages to the "ry" output channel, optionally mirroring them
 * to the console in a CI environment (e.g. GitHub Actions).
 *
 * Use this for messages intended for the user. The server's own stderr
 * is written to a separate channel (see `server.ts`).
 */
export const logger = new ExtensionLogger();

/**
 * A VS Code output channel that is lazily created when it is first
 * accessed.
 *
 * Used for the LSP trace channel, which is only needed when the user
 * enables trace logging. Avoids creating an empty output channel on
 * every activation.
 */
export class LazyOutputChannel implements vscode.OutputChannel {
  name: string;
  private _channel: vscode.OutputChannel | undefined;

  constructor(name: string) {
    this.name = name;
  }

  private get channel(): vscode.OutputChannel {
    if (!this._channel) {
      this._channel = vscode.window.createOutputChannel(this.name);
    }
    return this._channel;
  }

  append(value: string): void {
    this.channel.append(value);
  }

  appendLine(value: string): void {
    this.channel.appendLine(value);
  }

  replace(value: string): void {
    this.channel.replace(value);
  }

  clear(): void {
    this._channel?.clear();
  }

  show(preserveFocus?: boolean): void;
  show(column?: vscode.ViewColumn, preserveFocus?: boolean): void;
  show(column?: unknown, preserveFocus?: unknown): void {
    this.channel.show(column as vscode.ViewColumn, preserveFocus as boolean);
  }

  hide(): void {
    this._channel?.hide();
  }

  dispose(): void {
    this._channel?.dispose();
  }
}
