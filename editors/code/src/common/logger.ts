import * as util from "util";
import * as vscode from "vscode";

class ExtensionLogger {
  readonly channel = vscode.window.createOutputChannel("ry", { log: true });

  private readonly isCI = process.env.CI === "true";

  private logForCI(...messages: unknown[]): void {
    if (this.isCI) {
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

// Public logging surface; tests can provide a stub.
export type Logger = Pick<
  ExtensionLogger,
  "channel" | "error" | "warn" | "info" | "debug" | "trace"
>;

// Client logs, mirrored to the console in CI. Server stderr uses its own channel.
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
  show(column?: vscode.ViewColumn | boolean, preserveFocus?: boolean): void {
    if (typeof column === "boolean") {
      this.channel.show(column);
    } else {
      this.channel.show(column, preserveFocus);
    }
  }

  hide(): void {
    this._channel?.hide();
  }

  dispose(): void {
    this._channel?.dispose();
  }
}
