"use strict";

// Stub for the `which` package, used by tier 2 (PATH lookup) of
// resolveServerBinary.
//
// With the real module these tests depend on the host: a developer who has
// iris-agentic-dev installed gets an early return from tier 2, so the cache
// and download paths that tiers 3+ cover never run, and assertions about them
// fail. Default is "nothing on PATH"; a test that wants a PATH hit sets
// `found` explicitly.

let found = null;

function which(_name) {
  if (found) return found;
  const err = new Error("not found");
  err.code = "ENOENT";
  throw err;
}

which.sync = which;

// Test controls
which.__setFound = (p) => {
  found = p;
};
which.__reset = () => {
  found = null;
};

module.exports = which;
module.exports.default = which;
