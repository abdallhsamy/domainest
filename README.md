![Domainest](logo.png)

# Domainest

Domainest is a tray/menu-bar dev tool that replaces “`pnpm dev` + localhost” with **human-friendly local domains**, e.g.:

- `https://myapp.test`
- `https://admin.test`
- `http://legacy.test` (when SSL is disabled per project)

It’s built with **Tauri (Rust)** + **Vue 3 (Vite)**, and ships a fully local stack:

- **Caddy (sidecar)**: reverse proxy + routing per domain
- **mkcert (sidecar)**: locally-trusted CA + per-domain certificates
- **Embedded DNS server**: answers `*.suffix` → `127.0.0.1` (macOS uses `/etc/resolver/<suffix>`)

## Features

- **Menu bar icon** with quick actions:
  - **Projects**, **Add Project**, **Settings**, **Quit**
  - Per-project submenu: **Start/Stop**, **Open in browser**
- **Dashboard**
  - List / add / edit / remove projects
  - Toggle SSL (mkcert) per project
  - Start/Stop + Open
  - View **live logs** for running projects
- **Correct process management**
  - Dev servers run in their own process group so **Stop terminates the full process tree**
- **Multiple projects**
  - Each project gets a **unique port** by default (3000, 3001, 3002…)
  - The dev server is started with flags/env so it **binds to the configured port**

## How it works (high level)

1. **DNS**: your chosen suffix (default `test`) resolves to `127.0.0.1`.
2. **Caddy**: routes `https://<domain>` → `http://localhost:<port>` for each running project.
3. **mkcert**: generates & reuses certs at `~/.domainest/certs/<domain>.pem`.
4. **Dev server**: Domainest spawns your configured command (default `pnpm dev`) in the project directory.

## Requirements

- **pnpm** (your projects are assumed to use it by default; you can change the command per project)
- **Rust toolchain** (for local dev of Domainest itself)

No system `caddy`, `mkcert`, or `dnsmasq` install is required: Domainest bundles what it needs.

## Install & run (development)

Install dependencies:

```bash
pnpm install
```

Optional — use the repo’s Git hooks so `cargo fmt --check` runs before each commit (matches CI; no extra npm packages):

```bash
git config core.hooksPath .githooks
```

Download bundled sidecars:

```bash
pnpm setup:deps
```

Run the app:

```bash
pnpm tauri:dev
```

## Build (production)

```bash
pnpm tauri:build
```

## Usage

### Add a project

- From the **menu bar icon**: **Add Project**
- Or from the dashboard: **Add project** → choose a folder

Default values:

- **domain**: `folder-name.<suffix>` (suffix defaults to `test`)
- **port**: first available in `3000..`
- **command/args**: `pnpm dev`
- **ssl**: enabled

### Start/Stop

- Use the project card button in the dashboard or the tray submenu.
- Stop will terminate the full `pnpm → node → watcher` tree.

### View logs (“terminal”)

- For running projects, click **Logs** on the project card.
- Logs are read from: `~/.domainest/logs/<project-id>.log`

### Change DNS zone

Open **Settings → DNS zone**.

macOS **split-DNS** only sends queries for the zone you configure to Domainest:

| Zone you set | Resolver file | What resolves locally | What stays on normal DNS |
|--------------|---------------|------------------------|---------------------------|
| `test` | `/etc/resolver/test` | `*.test` (e.g. `app.test`) | N/A (`.test` is for testing) |
| `myapp.com` | `/etc/resolver/myapp.com` | `*.myapp.com` only | `github.com`, `google.com`, etc. |

- Examples: `test`, `myapp`, `myapp.com` (with or without a leading dot)
- New projects default to `name.<zone>` (e.g. `api.myapp.com` when the zone is `myapp.com`)

**Blocked as global zone:** single-label `dev`, `com`, `app`, etc. (they hijack the whole TLD).

**Per-project `.dev` names** (e.g. `be-brand.dev` while the zone stays `test`): Domainest installs `/etc/resolver/be-brand.dev` so only that hostname uses local DNS; `github.dev` and other real `.dev` sites are unaffected.

On startup, Domainest removes stale `/etc/resolver/*` files that point at its DNS (127.0.0.1:53535) except for your active zone.

## Data locations

If you used an older build that stored data under `~/.dev-domains`, copy or move that folder to `~/.domainest` (or re-add projects in the app).

- **Projects**: `~/.domainest/projects.json`
- **App state** (mkcert installed, suffix, etc.): `~/.domainest/state.json`
- **Certificates**: `~/.domainest/certs/`
- **Logs**: `~/.domainest/logs/`
- **Caddy runtime**: `~/.domainest/caddy/`
- **Caddyfile**: `~/.domainest/Caddyfile`

## Troubleshooting

### “Only the first project works”

This usually happens when a dev server binds to a different port than expected.
Domainest now:

- assigns unique ports per project
- forces dev servers to bind to the project’s configured port

If you changed your dev command manually, ensure it honors:

- `-- --port <port>` (for Vite/Nuxt/etc)
- or `PORT=<port>` env

### Real `.dev` / `.app` sites stopped working

Usually caused by an old `/etc/resolver/dev` (or similar) from a previous suffix. Domainest now blocks those suffixes and prunes leftover resolver files on launch. You can also remove them manually:

```bash
sudo rm /etc/resolver/dev /etc/resolver/app /etc/resolver/local
sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
```

### DNS doesn’t resolve `*.suffix`

macOS uses `/etc/resolver/<suffix>`. Verify it exists:

```bash
cat /etc/resolver/<suffix>
```

Expected content:

```text
nameserver 127.0.0.1
port 53535
```

### HTTPS certificate issues

- Certs live in `~/.domainest/certs/`
- First run performs `mkcert -install` (idempotent)

### Stale routes after crashes

Domainest rewrites the managed block in `~/.domainest/Caddyfile` and reloads Caddy on startup and project state changes.

## Project structure

- `src/`: Vue 3 dashboard + services
- `src-tauri/`: Rust backend + sidecars
  - `src-tauri/src/services/`: process manager, Caddy manager, mkcert manager, DNS server


