"use strict";

// The MCP provider under a stubbed `vscode`. These cover what a cloned
// repository can do to it: the workspace is untrusted, or it carries its own
// `intersystems.servers` entry for a name the user already has.

const { test, beforeEach } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const Module = require("node:module");

const STUB = require.resolve("./stubs/vscode.cjs");
const WHICH = require.resolve("./stubs/which.cjs");
const _orig = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  if (request === "vscode") return STUB;
  if (request === "which") return WHICH;
  return _orig.call(this, request, ...rest);
};

const vscode = require("vscode");
const { IrisDevMcpProvider } = require("../.test-out/extension.cjs");
const pkg = require("../package.json");

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-ext-"));
const fakeBinary = path.join(tmp, "iris-agentic-dev");
fs.writeFileSync(fakeBinary, "#!/bin/sh\necho ok\n", { mode: 0o755 });

const USER_PROD = { webServer: { host: "iris.corp.example", port: 443, scheme: "https" }, username: "alice" };
const WS_PROD = { webServer: { host: "attacker.example", port: 80 }, username: "alice" };

const sectionsRead = [];
// `conn` is one flat object set at the user level. `connScopes` sets the
// layers apart, as VS Code reports them: `get` returns them merged, `inspect`
// returns each one.
function useConfig({ conn, connScopes, user, workspace, iad = {} }) {
  const layers = connScopes ?? { globalValue: conn };
  const merged = Object.values(layers).some((l) => l !== undefined)
    ? {
        ...layers.defaultValue,
        ...layers.globalValue,
        ...layers.workspaceValue,
        ...layers.workspaceFolderValue,
      }
    : undefined;
  vscode.workspace.workspaceFolders = [{ uri: { scheme: "file" } }];
  vscode.workspace.getConfiguration = (section) => {
    sectionsRead.push(section);
    if (section === "objectscript") {
      return {
        get: (k) => (k === "conn" ? merged : undefined),
        inspect: (k) => (k === "conn" ? layers : undefined),
      };
    }
    if (section === "intersystems") {
      return {
        get: (k) => (k === "servers" ? { ...user, ...workspace } : undefined),
        inspect: (k) =>
          k === "servers" ? { globalValue: user, workspaceValue: workspace } : undefined,
      };
    }
    if (section === "iris-agentic-dev") {
      return { get: (k, d) => ({ serverPath: fakeBinary, ...iad })[k] ?? d };
    }
    return { get: (_k, d) => d };
  };
}

function context() {
  return { extension: { packageJSON: pkg }, globalStorageUri: { fsPath: tmp } };
}

const token = { isCancellationRequested: false };

beforeEach(() => {
  vscode.workspace.isTrusted = true;
  vscode.__resetShown();
  vscode.__resetLog();
  vscode.extensions.getExtension = () => undefined;
  sectionsRead.length = 0;
});

test("package.json declares limited support for untrusted workspaces", () => {
  const cap = pkg.capabilities && pkg.capabilities.untrustedWorkspaces;
  assert.ok(cap, "capabilities.untrustedWorkspaces must be declared");
  assert.equal(cap.supported, "limited");
  assert.ok(typeof cap.description === "string" && cap.description.length > 20);
});

test("serverPath is machine-scoped, so a workspace cannot set it", () => {
  const prop = pkg.contributes.configuration.properties["iris-agentic-dev.serverPath"];
  assert.equal(prop.scope, "machine");
});

test("an untrusted workspace starts nothing and reads no workspace connection or servers", async () => {
  vscode.workspace.isTrusted = false;
  useConfig({ conn: { active: true, server: "prod" }, user: { prod: USER_PROD }, workspace: { prod: WS_PROD } });
  const provider = new IrisDevMcpProvider(context());
  try {
    const defs = await provider.provideMcpServerDefinitions(token);
    assert.deepEqual(defs, []);
    assert.deepEqual(sectionsRead, [], "no settings are read before trust is granted");
    assert.equal(
      vscode.__log.info.some((l) => /binary path/.test(l)),
      false,
      "the server binary is not even resolved"
    );
  } finally {
    provider.dispose();
  }
});

test("granting trust re-offers the server definitions, and dispose stops listening", async () => {
  vscode.workspace.isTrusted = false;
  useConfig({ conn: { active: true }, user: {}, workspace: {} });
  const provider = new IrisDevMcpProvider(context());
  let fired = 0;
  provider.onDidChangeMcpServerDefinitions(() => fired++);
  assert.equal(vscode.__handlerCounts().trust, 1);
  vscode.__grantTrust();
  assert.equal(fired, 1);
  const defs = await provider.provideMcpServerDefinitions(token);
  assert.equal(defs.length, 1, "trusted now: the server is offered");
  provider.dispose();
  assert.equal(vscode.__handlerCounts().trust, 0);
});

test("a user-level server definition wins over a workspace one of the same name", async () => {
  useConfig({ conn: { active: true, server: "prod" }, user: { prod: USER_PROD }, workspace: { prod: WS_PROD } });
  const provider = new IrisDevMcpProvider(context());
  try {
    const [def] = await provider.provideMcpServerDefinitions(token);
    assert.equal(def.env.IRIS_HOST, "iris.corp.example");
    assert.equal(def.env.IRIS_WEB_PORT, 443);
    assert.equal(def.env.IRIS_SCHEME, "https");
    assert.match(vscode.__log.warn.join("\n"), /workspace.*prod.*ignored/i);
  } finally {
    provider.dispose();
  }
});

function fakeServerManager(calls) {
  return {
    isActive: true,
    activate: async () => calls.push("activate"),
    exports: {
      getServerSpec: async (name) => {
        calls.push(`getServerSpec:${name}`);
        return { name, password: "stored-secret" };
      },
    },
  };
}

test("the stored Server Manager password goes to a user-level definition", async () => {
  useConfig({ conn: { active: true, server: "prod" }, user: { prod: USER_PROD }, workspace: { prod: WS_PROD } });
  const calls = [];
  vscode.extensions.getExtension = () => fakeServerManager(calls);
  const provider = new IrisDevMcpProvider(context());
  try {
    const [def] = await provider.provideMcpServerDefinitions(token);
    const out = await provider.resolveMcpServerDefinition(def, token);
    assert.equal(out.env.IRIS_PASSWORD, "stored-secret");
    assert.equal(out.env.IRIS_HOST, "iris.corp.example");
  } finally {
    provider.dispose();
  }
});

test("a workspace-only definition never receives the stored password", async () => {
  useConfig({ conn: { active: true, server: "prod" }, user: {}, workspace: { prod: WS_PROD } });
  const calls = [];
  vscode.extensions.getExtension = () => fakeServerManager(calls);
  const provider = new IrisDevMcpProvider(context());
  try {
    const [def] = await provider.provideMcpServerDefinitions(token);
    assert.equal(def.env.IRIS_HOST, "attacker.example");
    const out = await provider.resolveMcpServerDefinition(def, token);
    assert.deepEqual(calls, [], "Server Manager is not asked for a secret");
    assert.equal(out.env.IRIS_PASSWORD, undefined);
    assert.equal(vscode.__shown.inputBox.length, 1, "the user is asked instead");
    assert.match(vscode.__shown.inputBox[0].prompt, /attacker\.example/, "the prompt names the host");
  } finally {
    provider.dispose();
  }
});

test("a workspace definition that carries its own password keeps it", async () => {
  const own = { ...WS_PROD, password: "from-the-file" };
  useConfig({ conn: { active: true, server: "dev" }, user: {}, workspace: { dev: own } });
  const calls = [];
  vscode.extensions.getExtension = () => fakeServerManager(calls);
  const provider = new IrisDevMcpProvider(context());
  try {
    const [def] = await provider.provideMcpServerDefinitions(token);
    const out = await provider.resolveMcpServerDefinition(def, token);
    assert.equal(out.env.IRIS_PASSWORD, "from-the-file");
    assert.deepEqual(calls, []);
  } finally {
    provider.dispose();
  }
});

// --- objectscript.conn: the user's password goes only to the user's host ------

const USER_CONN = {
  active: true,
  host: "iris.corp.example",
  port: 443,
  ns: "USER",
  username: "alice",
  password: "user-level-secret",
};

async function launch(connScopes, { user = {}, workspace = {} } = {}) {
  useConfig({ connScopes, user, workspace });
  const provider = new IrisDevMcpProvider(context());
  try {
    const [def] = await provider.provideMcpServerDefinitions(token);
    return { def, out: def && (await provider.resolveMcpServerDefinition(def, token)) };
  } finally {
    provider.dispose();
  }
}

test("a workspace that moves host and port does not inherit the user-level password", async () => {
  const { def, out } = await launch({
    globalValue: USER_CONN,
    workspaceValue: { host: "attacker.example", port: 80 },
  });
  assert.equal(def.env.IRIS_HOST, "attacker.example");
  assert.equal(def.env.IRIS_PASSWORD, undefined);
  assert.equal(out.env.IRIS_PASSWORD, undefined);
  assert.equal(vscode.__shown.inputBox.length, 1, "the user is asked instead");
  assert.match(vscode.__shown.inputBox[0].prompt, /attacker\.example/);
  assert.doesNotMatch(JSON.stringify(vscode.__log), /user-level-secret/);
});

test("a workspace folder that moves only the port does not inherit the password either", async () => {
  const { def } = await launch({
    globalValue: USER_CONN,
    workspaceFolderValue: { port: 8080 },
  });
  assert.equal(def.env.IRIS_PASSWORD, undefined);
});

test("a workspace that moves only the host does not inherit the password", async () => {
  const { def } = await launch({
    globalValue: USER_CONN,
    workspaceValue: { host: "attacker.example" },
  });
  assert.equal(def.env.IRIS_PASSWORD, undefined);
});

test("a workspace that changes only the namespace keeps the user-level password", async () => {
  const { def } = await launch({
    globalValue: USER_CONN,
    workspaceValue: { ns: "SCRATCH" },
  });
  assert.equal(def.env.IRIS_PASSWORD, "user-level-secret");
  assert.equal(def.env.IRIS_NAMESPACE, "SCRATCH");
});

test("a workspace that repeats the user's host and port keeps the password", async () => {
  const { def } = await launch({
    globalValue: USER_CONN,
    workspaceValue: { host: "IRIS.corp.example", port: 443 },
  });
  assert.equal(def.env.IRIS_PASSWORD, "user-level-secret");
});

test("a workspace conn that carries its own password uses that one", async () => {
  const { def } = await launch({
    globalValue: USER_CONN,
    workspaceValue: { host: "ci.example", port: 80, password: "from-the-file" },
  });
  assert.equal(def.env.IRIS_PASSWORD, "from-the-file");
});

test("a workspace that names another server does not inherit the password", async () => {
  const { def } = await launch(
    { globalValue: { ...USER_CONN, server: "prod" }, workspaceValue: { server: "other" } },
    { user: { prod: USER_PROD, other: USER_PROD } }
  );
  assert.equal(def.env.IRIS_SERVER_NAME, "other");
  assert.equal(def.env.IRIS_PASSWORD, undefined);
});

test("a workspace-only named server gets no user-level conn password", async () => {
  const { def, out } = await launch(
    { globalValue: { ...USER_CONN, server: "dev" } },
    { workspace: { dev: WS_PROD } }
  );
  assert.equal(def.env.IRIS_HOST, "attacker.example");
  assert.equal(def.env.IRIS_PASSWORD, undefined);
  assert.equal(out.env.IRIS_PASSWORD, undefined);
  assert.equal(vscode.__shown.inputBox.length, 1, "the user is asked instead");
});

test("a user-level named server keeps the user-level conn password", async () => {
  const { def } = await launch(
    { globalValue: { ...USER_CONN, server: "prod" } },
    { user: { prod: USER_PROD } }
  );
  assert.equal(def.env.IRIS_HOST, "iris.corp.example");
  assert.equal(def.env.IRIS_PASSWORD, "user-level-secret");
});

test("a conn set only in the workspace is the workspace's own, password included", async () => {
  const { def } = await launch({
    workspaceValue: { active: true, host: "ci.example", port: 80, password: "ws-own" },
  });
  assert.equal(def.env.IRIS_PASSWORD, "ws-own");
});

test("a conn set only in the workspace has no user-level password to inherit", async () => {
  const { def } = await launch({
    workspaceValue: { active: true, host: "ci.example", port: 80 },
  });
  assert.equal(def.env.IRIS_PASSWORD, undefined);
});
