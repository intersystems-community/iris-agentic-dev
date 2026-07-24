import * as fs from "fs";
import * as https from "https";
import * as path from "path";

const MAX_REDIRECTS = 10;

export function downloadBinary(
  url: string,
  dest: string,
  onProgress: (fraction: number) => void
): Promise<void> {
  return new Promise((resolve, reject) => {
    const tmp = dest + ".tmp";
    // Ensure destination directory exists
    fs.mkdirSync(path.dirname(dest), { recursive: true });

    let redirectsLeft = MAX_REDIRECTS;

    function fetch(currentUrl: string): void {
      https
        .get(currentUrl, (res) => {
          if (
            res.statusCode !== undefined &&
            res.statusCode >= 300 &&
            res.statusCode < 400 &&
            res.headers.location
          ) {
            if (--redirectsLeft < 0) {
              reject(new Error("Too many redirects"));
              return;
            }
            fetch(res.headers.location);
            return;
          }

          if (!res.statusCode || res.statusCode < 200 || res.statusCode >= 300) {
            reject(new Error(`HTTP ${res.statusCode} downloading ${currentUrl}`));
            return;
          }

          const total = parseInt(res.headers["content-length"] ?? "0", 10);
          let received = 0;
          const out = fs.createWriteStream(tmp);

          res.on("data", (chunk: Buffer) => {
            received += chunk.length;
            if (total > 0) onProgress(received / total);
          });

          res.pipe(out);

          out.on("finish", () => {
            out.close(() => {
              fs.rename(tmp, dest, (err) => {
                if (err) reject(err);
                else resolve();
              });
            });
          });

          out.on("error", (err) => {
            fs.unlink(tmp, () => reject(err));
          });

          res.on("error", (err) => {
            fs.unlink(tmp, () => reject(err));
          });
        })
        .on("error", reject);
    }

    fetch(url);
  });
}
