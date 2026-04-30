import { createReadStream, createWriteStream, existsSync, mkdirSync } from "node:fs";
import { chmod, rename } from "node:fs/promises";
import { basename, join } from "node:path";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";
import { createGunzip } from "node:zlib";
// import tar from "tar";
import * as tar from "tar";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

const CADDY_VERSION = process.env.CADDY_VERSION ?? "2.11.2";
const OUT_DIR = join(__dirname, "..", "src-tauri", "bin");

const platform = process.platform; // 'darwin' | 'linux'
const arch = process.arch; // 'arm64' | 'x64'

if (platform !== "darwin" && platform !== "linux") {
  throw new Error(`Unsupported platform: ${platform}`);
}
if (arch !== "arm64" && arch !== "x64") {
  throw new Error(`Unsupported arch: ${arch}`);
}

const triple = toTargetTriple(platform, arch);
const asset = `caddy_${CADDY_VERSION}_${platform === "darwin" ? "mac" : "linux"}_${
  arch === "x64" ? "amd64" : "arm64"
}.tar.gz`;
const url = `https://github.com/caddyserver/caddy/releases/download/v${CADDY_VERSION}/${asset}`;

mkdirSync(OUT_DIR, { recursive: true });
const tmpTgz = join(OUT_DIR, asset);
const outBin = join(OUT_DIR, `caddy-${triple}`);

if (existsSync(outBin)) {
  console.log(`Caddy already present: ${outBin}`);
  process.exit(0);
}

console.log(`Downloading ${url}`);
await download(url, tmpTgz);

console.log(`Extracting ${basename(tmpTgz)}`);
await pipeline(createReadStream(tmpTgz), createGunzip(), tar.x({ cwd: OUT_DIR, filter: (p) => p === "caddy" }));

await rename(join(OUT_DIR, "caddy"), outBin);
await chmod(outBin, 0o755);
console.log(`Installed: ${outBin}`);

async function download(fromUrl, toPath) {
  const res = await fetch(fromUrl);
  if (!res.ok || !res.body) {
    throw new Error(`Failed to download ${fromUrl}: ${res.status} ${res.statusText}`);
  }
  const file = createWriteStream(toPath);
  await pipeline(res.body, file);
}

function toTargetTriple(p, a) {
  if (p === "darwin") {
    return a === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin";
  }
  return a === "arm64" ? "aarch64-unknown-linux-gnu" : "x86_64-unknown-linux-gnu";
}

