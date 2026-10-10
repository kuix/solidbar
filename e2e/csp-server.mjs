// Serves a build of the UI (debug for the UI tests, release for the release test) with the
// Content-Security-Policy from tauri.conf.json, completed the way Tauri completes it (hashes of
// the inline scripts), so a policy that would leave the packaged app blank fails a test instead
// of shipping. `trunk serve` and `cargo tauri dev` don't apply the policy at all.
//
// Usage: node csp-server.mjs <dist dir> <port>

import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";

const [distArg, port] = process.argv.slice(2);
const dist = path.resolve(distArg);
const config = JSON.parse(await readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
const index = await readFile(path.join(dist, "index.html"), "utf8");

const hashes = [...index.matchAll(/<script(?![^>]*\ssrc=)[^>]*>([\s\S]*?)<\/script>/g)].map(
  ([, body]) => `'sha256-${createHash("sha256").update(body).digest("base64")}'`,
);
const csp = config.app.security.csp.replace(/script-src ([^;]*)/, `script-src $1 ${hashes.join(" ")}`);

const types = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".css": "text/css",
};

createServer(async (request, response) => {
  const pathname = new URL(request.url, "http://localhost").pathname;
  const file = path.join(dist, pathname === "/" ? "index.html" : path.normalize(pathname));
  if (!file.startsWith(dist)) {
    response.writeHead(403).end();
    return;
  }
  try {
    const body = await readFile(file);
    response.writeHead(200, {
      "Content-Type": types[path.extname(file)] ?? "application/octet-stream",
      "Content-Security-Policy": csp,
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
}).listen(Number(port), "127.0.0.1");
