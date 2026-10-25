"use strict";

// Minimal `vscode` module stub for unit tests.
//
// managedInstall.ts is bundled with `--external:vscode`, so requiring it pulls
// in the real module, which only exists inside the VS Code extension host.
// This stands in for it.
//
// This used to live in node_modules/vscode/, created by hand. npm ci wipes
// node_modules, so on CI the require failed and managedInstall.test.cjs did
// not run at all — it only ever passed on machines where someone had made the
// stub locally. Keeping it in test/ means CI runs the same tests as a laptop.

// Log lines are recorded so tests can assert on what the user is told. A
// warning nobody can see is the bug this exists to catch, so "did it log" is
// itself the assertion.
const logged = { info: [], warn: [], error: [] };

// --- Surface used by extension.ts (the MCP provider) -------------------------
class EventEmitter {
  constructor() {
    this.handlers = [];
    this.event = (h) => {
      this.handlers.push(h);
      return { dispose: () => (this.handlers = this.handlers.filter((x) => x !== h)) };
    };
  }
  fire(v) {
    for (const h of [...this.handlers]) h(v);
  }
  dispose() {
    this.handlers = [];
  }
}

class McpStdioServerDefinition {
  constructor(label, command, args) {
    this.label = label;
    this.command = command;
    this.args = args;
    this.env = {};
  }
}

const trust = new EventEmitter();
const configChange = new EventEmitter();
const shown = { warning: [], error: [], information: [], inputBox: [] };

const stub = {
  EventEmitter,
  McpStdioServerDefinition,
  __shown: shown,
  __grantTrust: () => {
    stub.workspace.isTrusted = true;
    trust.fire();
  },
  __fireConfigChange: (affects) => configChange.fire({ affectsConfiguration: affects }),
  __handlerCounts: () => ({ trust: trust.handlers.length, config: configChange.handlers.length }),
  __resetShown: () => {
    for (const k of Object.keys(shown)) shown[k].length = 0;
  },
  __log: logged,
  __resetLog: () => {
    logged.info.length = 0;
    logged.warn.length = 0;
    logged.error.length = 0;
  },
  window: {
    createOutputChannel: () => ({
      info: (m) => logged.info.push(String(m)),
      warn: (m) => logged.warn.push(String(m)),
      error: (m) => logged.error.push(String(m)),
      show: () => {},
      dispose: () => {},
    }),
    showWarningMessage: async (m) => void shown.warning.push(String(m)),
    showInputBox: async (o) => {
      shown.inputBox.push(o);
      return undefined;
    },
    withProgress: async (_opts, task) => task({ report: () => {} }),
    showErrorMessage: async () => {},
    showInformationMessage: async (m) => void shown.information.push(String(m)),
  },
  workspace: {
    getConfiguration: () => ({ get: () => "", inspect: () => undefined }),
    isTrusted: true,
    workspaceFolders: undefined,
    onDidGrantWorkspaceTrust: (h) => trust.event(h),
    onDidChangeConfiguration: (h) => configChange.event(h),
  },
  extensions: { getExtension: () => undefined },
  authentication: { getSession: async () => undefined },
  ProgressLocation: { Notification: 15 },
  Uri: { file: (p) => ({ fsPath: p }) },
};

module.exports = stub;
