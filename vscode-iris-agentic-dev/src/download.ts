import * as crypto from "crypto";
import * as fs from "fs";
import * as http from "http";
import * as https from "https";
import * as path from "path";
import { Transform, pipeline } from "stream";

export type DownloadErrorCode =
  | "BAD_HASH"
  | "BAD_URL"
  | "BAD_REDIRECT"
  | "TOO_MANY_REDIRECTS"
  | "HTTP_STATUS"
  | "SIZE_LIMIT"
  | "LENGTH_MISMATCH"
  | "HASH_MISMATCH"
  | "IO";

/** Every failure the downloader reports carries a code a caller can branch on. */
export class DownloadError extends Error {
  constructor(
    public readonly code: DownloadErrorCode,
    message: string,
  ) {
    super(message);
    this.name = "DownloadError";
  }
}

export interface DownloadPolicy {
  /** Exact host names (no suffix matching) the start URL and every redirect may name. */
  readonly allowedHosts: readonly string[];
  readonly protocols: readonly string[];
  readonly maxRedirects: number;
  readonly maxBytes: number;
  /** A response that goes quiet for this long is abandoned. */
  readonly idleTimeoutMs: number;
}

/**
 * GitHub serves a release asset from github.com, which answers 302 to
 * release-assets.githubusercontent.com (objects.githubusercontent.com before
 * 2025). Nothing else is a release download host, so nothing else is followed:
 * a redirect is a server-supplied URL and must not be able to point the
 * extension at an address of its choosing.
 */
export const RELEASE_DOWNLOAD_POLICY: DownloadPolicy = {
  allowedHosts: [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
  ],
  protocols: ["https:"],
  maxRedirects: 5,
  // The largest release asset is a few tens of MB; this leaves room to grow
  // while stopping a hostile or broken server from filling the disk.
  maxBytes: 256 * 1024 * 1024,
  idleTimeoutMs: 30_000,
};

const SHA256_HEX = /^[0-9a-f]{64}$/;

/** Parses `raw` and checks it against the policy, or throws `code`. */
function checkedUrl(
  raw: string,
  base: string | undefined,
  policy: DownloadPolicy,
  code: "BAD_URL" | "BAD_REDIRECT",
): URL {
  let url: URL;
  try {
    url = new URL(raw, base);
  } catch {
    throw new DownloadError(
      code,
      `not a valid URL: ${JSON.stringify(raw.slice(0, 200))}`,
    );
  }
  if (!policy.protocols.includes(url.protocol)) {
    throw new DownloadError(
      code,
      `${url.protocol} is not an allowed scheme (${url.origin})`,
    );
  }
  if (url.username || url.password) {
    throw new DownloadError(
      code,
      `a URL with credentials is not followed (${url.host})`,
    );
  }
  if (!policy.allowedHosts.includes(url.hostname.toLowerCase())) {
    throw new DownloadError(
      code,
      `${url.hostname} is not a GitHub release download host`,
    );
  }
  return url;
}

/**
 * Downloads `url` to `dest` only if the bytes hash to `expectedSha256`.
 *
 * The body goes to `dest + ".tmp"` and is renamed over `dest` after the hash,
 * the length and the size cap have all passed; on any failure the temp file is
 * removed and `dest` is left as it was. The hash is the whole defence: HTTPS
 * plus a release asset that can be replaced is not enough to run a binary.
 */
export function downloadBinary(
  url: string,
  dest: string,
  onProgress: (fraction: number) => void,
  expectedSha256: string,
  policy: DownloadPolicy = RELEASE_DOWNLOAD_POLICY,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const expected = expectedSha256.toLowerCase();
    if (!SHA256_HEX.test(expected)) {
      reject(
        new DownloadError(
          "BAD_HASH",
          "the expected SHA-256 is not 64 hex digits",
        ),
      );
      return;
    }
    let first: URL;
    try {
      first = checkedUrl(url, undefined, policy, "BAD_URL");
    } catch (err) {
      reject(err);
      return;
    }

    const tmp = dest + ".tmp";
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.rmSync(tmp, { force: true });

    let settled = false;
    const fail = (err: Error): void => {
      if (settled) return;
      settled = true;
      fs.rm(tmp, { force: true }, () => reject(err));
    };

    let redirectsLeft = policy.maxRedirects;

    function get(current: URL): void {
      const client = current.protocol === "http:" ? http : https;
      const req = client.get(current, (res) => {
        const status = res.statusCode ?? 0;

        if (status >= 300 && status < 400 && res.headers.location) {
          res.resume();
          if (--redirectsLeft < 0) {
            fail(
              new DownloadError(
                "TOO_MANY_REDIRECTS",
                `more than ${policy.maxRedirects} redirects`,
              ),
            );
            return;
          }
          let next: URL;
          try {
            next = checkedUrl(
              res.headers.location,
              current.href,
              policy,
              "BAD_REDIRECT",
            );
          } catch (err) {
            fail(err as Error);
            return;
          }
          get(next);
          return;
        }

        if (status < 200 || status >= 300) {
          res.resume();
          fail(
            new DownloadError(
              "HTTP_STATUS",
              `HTTP ${status} downloading ${current.origin}${current.pathname}`,
            ),
          );
          return;
        }

        const declared = res.headers["content-length"];
        const total = declared === undefined ? 0 : Number(declared);
        if (
          declared !== undefined &&
          (!Number.isSafeInteger(total) || total < 0)
        ) {
          res.resume();
          fail(
            new DownloadError(
              "LENGTH_MISMATCH",
              `unusable Content-Length ${JSON.stringify(declared)}`,
            ),
          );
          return;
        }
        if (total > policy.maxBytes) {
          res.resume();
          fail(
            new DownloadError(
              "SIZE_LIMIT",
              `Content-Length ${total} exceeds the ${policy.maxBytes} byte limit`,
            ),
          );
          return;
        }

        const hash = crypto.createHash("sha256");
        let received = 0;
        const meter = new Transform({
          transform(chunk: Buffer, _enc, cb) {
            received += chunk.length;
            if (received > policy.maxBytes) {
              cb(
                new DownloadError(
                  "SIZE_LIMIT",
                  `body exceeds the ${policy.maxBytes} byte limit`,
                ),
              );
              return;
            }
            hash.update(chunk);
            if (total > 0) onProgress(Math.min(1, received / total));
            cb(null, chunk);
          },
        });

        pipeline(
          res,
          meter,
          fs.createWriteStream(tmp, { flags: "wx" }),
          (err) => {
            if (settled) return;
            if (err) {
              // A connection cut short surfaces as an abort; say what it means.
              const cut = !(err instanceof DownloadError) && !res.complete;
              fail(
                cut
                  ? new DownloadError(
                      "LENGTH_MISMATCH",
                      `connection ended after ${received} bytes` +
                        (total > 0 ? ` of ${total}` : ""),
                    )
                  : err instanceof DownloadError
                    ? err
                    : new DownloadError("IO", err.message),
              );
              return;
            }
            if (total > 0 && received !== total) {
              fail(
                new DownloadError(
                  "LENGTH_MISMATCH",
                  `received ${received} bytes, Content-Length said ${total}`,
                ),
              );
              return;
            }
            if (!res.complete) {
              fail(
                new DownloadError(
                  "LENGTH_MISMATCH",
                  `connection ended after ${received} bytes`,
                ),
              );
              return;
            }
            const actual = hash.digest("hex");
            if (actual !== expected) {
              fail(
                new DownloadError(
                  "HASH_MISMATCH",
                  `SHA-256 is ${actual}, expected ${expected}`,
                ),
              );
              return;
            }
            fs.rename(tmp, dest, (renameErr) => {
              if (renameErr) {
                fail(new DownloadError("IO", renameErr.message));
                return;
              }
              settled = true;
              resolve();
            });
          },
        );
      });
      req.setTimeout(policy.idleTimeoutMs, () => {
        req.destroy(
          new DownloadError("IO", `no data for ${policy.idleTimeoutMs} ms`),
        );
      });
      req.on("error", (err) => {
        fail(
          err instanceof DownloadError
            ? err
            : new DownloadError("IO", err.message),
        );
      });
    }

    get(first);
  });
}
