"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const { selectNamedServer, mayUseStoredPassword } = require("../.test-out/serverSelection.cjs");

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
