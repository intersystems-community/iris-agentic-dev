"use strict";

// The downloader runs against a local HTTP server, never the network. A
// policy object is how the tests point it at 127.0.0.1: the production policy
// (https only, GitHub release hosts only) is asserted separately below, so a
// test-only policy cannot quietly become the shipped one.

const { test } = require("node:test");
const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");

const {
  downloadBinary,
  RELEASE_DOWNLOAD_POLICY,
} = require("../.test-out/download.cjs");

const sha256 = (buf) => crypto.createHash("sha256").update(buf).digest("hex");

const PAYLOAD = Buffer.from("#!/bin/sh\necho pretend-server\n".repeat(200));
const PAYLOAD_SHA = sha256(PAYLOAD);

function policy(overrides = {}) {
  return {
    allowedHosts: ["127.0.0.1"],
    protocols: ["http:"],
    maxRedirects: 3,
    maxBytes: 1024 * 1024,
    idleTimeoutMs: 5000,
    ...overrides,
  };
}

/** Starts a server; `handler` sees every request. Returns {url, hits, close}. */
async function serve(handler) {
  const hits = [];
  const server = http.createServer((req, res) => {
    hits.push(req.url);
    handler(req, res, server);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const port = server.address().port;
  return {
    port,
    hits,
    url: (p = "/bin") => `http://127.0.0.1:${port}${p}`,
    close: () =>
      new Promise((r) => {
        server.closeAllConnections?.();
        server.close(r);
      }),
  };
}

function tmpDest() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "iad-dl-"));
  return { dir, dest: path.join(dir, "sub", "server-bin") };
}

function leftovers(dir) {
  const out = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else out.push(p);
    }
  };
  walk(dir);
  return out;
}

async function rejectsWith(promise, code) {
  await assert.rejects(promise, (err) => {
    assert.equal(err.code, code, `expected ${code}, got ${err.code}: ${err.message}`);
    return true;
  });
}

test("production policy: https only, the GitHub release hosts, bounded", () => {
  assert.deepEqual(RELEASE_DOWNLOAD_POLICY.protocols, ["https:"]);
  assert.deepEqual(
    [...RELEASE_DOWNLOAD_POLICY.allowedHosts].sort(),
    ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"]
  );
  assert.ok(RELEASE_DOWNLOAD_POLICY.maxRedirects >= 1 && RELEASE_DOWNLOAD_POLICY.maxRedirects <= 5);
  assert.ok(RELEASE_DOWNLOAD_POLICY.maxBytes > 0 && RELEASE_DOWNLOAD_POLICY.maxBytes <= 512 * 1024 * 1024);
});

test("a matching hash installs the file and leaves no temp file", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(200, { "content-length": PAYLOAD.length });
    res.end(PAYLOAD);
  });
  const { dir, dest } = tmpDest();
  const fractions = [];
  try {
    await downloadBinary(srv.url(), dest, (f) => fractions.push(f), PAYLOAD_SHA, policy());
    assert.deepEqual(fs.readFileSync(dest), PAYLOAD);
    assert.deepEqual(leftovers(dir), [dest]);
    assert.equal(fractions[fractions.length - 1], 1);
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("an upper-case expected hash is accepted", async () => {
  const srv = await serve((_q, res) => res.end(PAYLOAD));
  const { dir, dest } = tmpDest();
  try {
    await downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA.toUpperCase(), policy());
    assert.ok(fs.existsSync(dest));
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a wrong hash deletes the download and never creates the destination", async () => {
  const srv = await serve((_q, res) => res.end(PAYLOAD));
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, sha256("something else"), policy()),
      "HASH_MISMATCH"
    );
    assert.deepEqual(leftovers(dir), []);
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a wrong hash does not replace an existing destination", async () => {
  const srv = await serve((_q, res) => res.end(PAYLOAD));
  const { dir, dest } = tmpDest();
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.writeFileSync(dest, "previous");
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, sha256("nope"), policy()),
      "HASH_MISMATCH"
    );
    assert.equal(fs.readFileSync(dest, "utf8"), "previous");
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("an expected hash that is not 64 hex digits is refused before any request", async () => {
  const srv = await serve((_q, res) => res.end(PAYLOAD));
  const { dir, dest } = tmpDest();
  try {
    for (const bad of ["", "abc", "z".repeat(64), PAYLOAD_SHA + "00"]) {
      await rejectsWith(downloadBinary(srv.url(), dest, () => {}, bad, policy()), "BAD_HASH");
    }
    assert.deepEqual(srv.hits, []);
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a start URL outside the policy is refused without a request", async () => {
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary("https://evil.example/iris-agentic-dev", dest, () => {}, PAYLOAD_SHA),
      "BAD_URL"
    );
    await rejectsWith(
      downloadBinary("http://github.com/x", dest, () => {}, PAYLOAD_SHA),
      "BAD_URL"
    );
    await rejectsWith(
      downloadBinary("https://github.com.evil.example/x", dest, () => {}, PAYLOAD_SHA),
      "BAD_URL"
    );
    await rejectsWith(
      downloadBinary("https://user:pw@github.com/x", dest, () => {}, PAYLOAD_SHA),
      "BAD_URL"
    );
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a redirect to a host outside the allow list is an error and is never requested", async () => {
  const target = await serve((_q, res) => res.end(PAYLOAD));
  const srv = await serve((_q, res) => {
    // 127.0.0.2 is not in the test policy's allow list.
    res.writeHead(302, { location: `http://127.0.0.2:${target.port}/bin` });
    res.end();
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy()),
      "BAD_REDIRECT"
    );
    assert.deepEqual(target.hits, []);
    assert.deepEqual(leftovers(dir), []);
  } finally {
    await srv.close();
    await target.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a redirect to another scheme is an error", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(302, { location: "ftp://127.0.0.1/bin" });
    res.end();
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy()),
      "BAD_REDIRECT"
    );
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a redirect with a malformed Location rejects instead of throwing", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(302, { location: "http://[not-an-ip/bin" });
    res.end();
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy()),
      "BAD_REDIRECT"
    );
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a redirect carrying credentials in the URL is an error", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(302, { location: "http://user:pw@127.0.0.1/bin" });
    res.end();
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy()),
      "BAD_REDIRECT"
    );
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a relative redirect stays on the allowed host and is followed", async () => {
  const srv = await serve((req, res) => {
    if (req.url === "/start") {
      res.writeHead(302, { location: "/real" });
      res.end();
    } else {
      res.end(PAYLOAD);
    }
  });
  const { dir, dest } = tmpDest();
  try {
    await downloadBinary(srv.url("/start"), dest, () => {}, PAYLOAD_SHA, policy());
    assert.deepEqual(fs.readFileSync(dest), PAYLOAD);
    assert.deepEqual(srv.hits, ["/start", "/real"]);
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a redirect loop stops after the policy's hop limit", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(302, { location: "/again" });
    res.end();
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy({ maxRedirects: 3 })),
      "TOO_MANY_REDIRECTS"
    );
    assert.equal(srv.hits.length, 4, "the first request plus three followed hops");
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a body shorter than Content-Length is an error and leaves nothing", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(200, { "content-length": PAYLOAD.length });
    res.write(PAYLOAD.subarray(0, 100));
    setTimeout(() => res.destroy(), 20);
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy()),
      "LENGTH_MISMATCH"
    );
    assert.deepEqual(leftovers(dir), []);
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a declared length over the cap is refused before the body is read", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(200, { "content-length": 10_000_000 });
    res.write(Buffer.alloc(1000));
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy({ maxBytes: 4096 })),
      "SIZE_LIMIT"
    );
    assert.deepEqual(leftovers(dir), []);
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a chunked body that runs past the cap is cut off and removed", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(200); // no Content-Length: chunked
    const chunk = Buffer.alloc(2048, 1);
    let sent = 0;
    const timer = setInterval(() => {
      if (res.destroyed || sent > 100) return clearInterval(timer);
      sent += 1;
      res.write(chunk);
    }, 2);
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy({ maxBytes: 8192 })),
      "SIZE_LIMIT"
    );
    assert.deepEqual(leftovers(dir), []);
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a non-2xx answer is an HTTP_STATUS error", async () => {
  const srv = await serve((_q, res) => {
    res.writeHead(404);
    res.end("nope");
  });
  const { dir, dest } = tmpDest();
  try {
    await rejectsWith(
      downloadBinary(srv.url(), dest, () => {}, PAYLOAD_SHA, policy()),
      "HTTP_STATUS"
    );
    assert.deepEqual(leftovers(dir), []);
  } finally {
    await srv.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
