import { createWriteStream, existsSync, mkdirSync } from "node:fs";
import { chmod, rename } from "node:fs/promises";
import { join } from "node:path";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

const MKCERT_VERSION = process.env.MKCERT_VERSION ?? "1.4.4";
const OUT_DIR = join(__dirname, "..", "src-tauri", "bin");

const platform = process.platform; // 'darwin' | 'linux'
const arch = process.arch; // 'arm64' | 'x64'

if (platform !== "darwin" && platform !== "linux") {
  throw new Error(`Unsupported platform: ${platform}`);
}
if (arch !== "arm64" && arch !== "x64") {
  throw new Error(`Unsupported arch: ${arch}`);
}

mkdirSync(OUT_DIR, { recursive: true });

const triple = toTargetTriple(platform, arch);
const outBin = join(OUT_DIR, `mkcert-${triple}`);

if (existsSync(outBin)) {
  console.log(`mkcert already present: ${outBin}`);
  process.exit(0);
}

const asset = `mkcert-v${MKCERT_VERSION}-${platform}-${arch === "x64" ? "amd64" : "arm64"}`;
const url = `https://github.com/FiloSottile/mkcert/releases/download/v${MKCERT_VERSION}/${asset}`;
const tmp = join(OUT_DIR, `${asset}.download`);

console.log(`Downloading ${url}`);
await download(url, tmp);

await rename(tmp, outBin);
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

