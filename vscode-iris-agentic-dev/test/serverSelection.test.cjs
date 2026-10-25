"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const { selectNamedServer, mayUseStoredPassword, connPassword, mergeConn } = require("../.test-out/serverSelection.cjs");

const userProd = { webServer: { host: "iris.corp.example", port: 443, scheme: "https" } };
const wsProd = { webServer: { host: "attacker.example", port: 80 } };

test("a user definition wins over a workspace definition of the same name", () => {
  const s = selectNamedServer("prod", { user: { prod: userProd }, workspace: { prod: wsProd } });
  assert.equal(s.server, userProd);
  assert.equal(s.source, "user");
  assert.equal(s.shadowedWorkspaceDefinition, true);
  assert.equal(mayUseStoredPassword(s), true);
});

test("a workspace-only definition is usable but never gets a stored password", () => {
  const s = selectNamedServer("prod", { user: { other: userProd }, workspace: { prod: wsProd } });
  assert.equal(s.server, wsProd);
  assert.equal(s.source, "workspace");
  assert.equal(mayUseStoredPassword(s), false);
});

test("a user-only definition is found when the workspace defines nothing", () => {
  const s = selectNamedServer("prod", { user: { prod: userProd }, workspace: undefined });
  assert.equal(s.source, "user");
  assert.equal(s.shadowedWorkspaceDefinition, false);
});

test("an unknown name selects nothing, including names that exist on every object", () => {
  const scopes = { user: { prod: userProd }, workspace: { x: wsProd } };
  for (const name of ["nope", "constructor", "__proto__", "toString", "hasOwnProperty"]) {
    assert.equal(selectNamedServer(name, scopes), null, name);
  }
  assert.equal(selectNamedServer("prod", { user: undefined, workspace: undefined }), null);
});

// --- objectscript.conn layers -------------------------------------------------

const USER = { active: true, host: "iris.corp.example", port: 443, password: "user-secret" };

test("mergeConn lays workspace over user and folder over workspace", () => {
  assert.deepEqual(
    mergeConn({ globalValue: USER, workspaceValue: { port: 80 }, workspaceFolderValue: { port: 8080 } }),
    { ...USER, port: 8080 }
  );
  assert.equal(mergeConn({}), undefined);
  assert.equal(mergeConn(undefined), undefined);
});

test("the user-level password needs the same host, port and server", () => {
  const pw = (ws, fromWs = false) => connPassword({ globalValue: USER, workspaceValue: ws }, fromWs);
  assert.equal(pw(undefined), "user-secret");
  assert.equal(pw({ ns: "X" }), "user-secret");
  assert.equal(pw({ host: "Iris.Corp.Example" }), "user-secret", "host names compare without case");
  assert.equal(pw({ host: "other.example" }), undefined);
  assert.equal(pw({ port: 80 }), undefined);
  assert.equal(pw({ server: "prod" }), undefined, "the user-level conn names no server");
  assert.equal(pw({ ns: "X" }, true), undefined, "never to a server defined only in the workspace");
});

test("defaults count: an unset host and port are localhost:52773 on both sides", () => {
  const user = { active: true, password: "user-secret" };
  assert.equal(connPassword({ globalValue: user, workspaceValue: { host: "localhost", port: 52773 } }, false), "user-secret");
  assert.equal(connPassword({ globalValue: user, workspaceValue: { host: "remote.example" } }, false), undefined);
});

test("a password written in the workspace conn is the workspace's own", () => {
  assert.equal(
    connPassword({ globalValue: USER, workspaceValue: { host: "ci.example", password: "ws-own" } }, false),
    "ws-own"
  );
  assert.equal(connPassword({ workspaceFolderValue: { password: "folder-own" } }, true), "folder-own");
});

test("no conn, no password", () => {
  assert.equal(connPassword(undefined, false), undefined);
  assert.equal(connPassword({ workspaceValue: { host: "ci.example" } }, false), undefined);
});
