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

const stub = {
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
    }),
    withProgress: async (_opts, task) => task({ report: () => {} }),
    showErrorMessage: async () => {},
    showInformationMessage: async () => {},
  },
  workspace: { getConfiguration: () => ({ get: () => "" }) },
  ProgressLocation: { Notification: 15 },
  Uri: { file: (p) => ({ fsPath: p }) },
};

module.exports = stub;
