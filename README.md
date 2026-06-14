# simd

Ultra-lightweight Markdown viewer. One binary, one container, zero bloat.

## Quick start

```bash
# Build
cargo build --release

# Run
SIMD_DATA_DIR=/path/to/docs SIMD_PORT=8080 ./target/release/simd

# Open
open http://localhost:8080
```

Or with Docker:

```bash
docker build -t simd .
docker run -p 8080:8080 -v /path/to/docs:/data -v simd-config:/config simd
```

## First run

On first launch you'll see a setup form. Enter a username and password — you are automatically signed in and redirected to the app. No double credential entry.

Session persists for 24 hours by default (configurable via `SIMD_SESSION_DURATION`).

## Auth flow

```
No config             → Setup page     → Create credentials
                                          ├─ Config saved to disk
                                          ├─ Session created (cookie)
                                          └─ Auto-redirect to app

Config exists         → Login page     → Enter credentials
  (no session)                           ├─ Session created (cookie)
                                         └─ Redirect to app

Config + session      → App (/)        → Full access

Config + Basic Auth   → App (/)        → Falls back to Basic header
  (API clients)                          for curl / scripts

Sign out              → Session cleared → Back to login page
```

## Usage

| Action | What happens |
|--------|-------------|
| Sign in | Enter credentials; session cookie stored for 24h |
| Click a folder ▸ | Expands tree + pre-caches all files in that folder |
| Click a file | Opens instantly from cache, rendered as Markdown |
| Toggle **MD** / **Raw** | Switch between rendered view and source text |
| Click ☀ / ☾ | Toggle light / dark theme (saved in localStorage) |
| Sign out | Clears session cookie, returns to login |

## Cache architecture

Three-tier caching, all lazy (no background processes):

| Layer | Type | Capacity | Invalidation |
|-------|------|----------|--------------|
| Server `FileCache` | LRU | 100 files | `mtime` — re-reads if file changed on disk |
| Server `TreeCache` | TTL | 1 entry | 30 seconds |
| Client `fileCache` | LRU | 50 files | In-memory, lives per tab |
| Browser HTTP | `Cache-Control` | Static pages | `max-age=3600` on all HTML |

**Pre-caching**: expanding a folder in the tree triggers a background `POST /api/cache-dir` — server reads + renders all Markdown files in that directory into `FileCache`. Subsequent file clicks are instant cache hits. Repeat calls skip already-cached files (`"skipped": N`).

## Features

- **Markdown rendering** via `pulldown-cmark` — headings, lists, tables, code blocks, blockquotes, strikethrough, task lists
- **File tree** — recursive, folders expand/collapse, dotfiles hidden, pre-caching on expand
- **Two viewing modes** — rendered MD or raw source
- **Two themes** — light and dark
- **Authentication** — bcrypt-hashed passwords, session-based (cookie) with configurable TTL; HTTP Basic Auth fallback for API clients
- **Auto-login after setup** — session created immediately on first-run setup, no double entry
- **Pre-caching** — expanding a folder pre-renders all its files server-side
- **Static page caching** — `Cache-Control: public, max-age=3600` on all HTML pages
- **Security** — directory traversal blocked via `canonicalize()`, all access confined to `/data`

## API

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| GET | `/api/check` | No | `{ setup_needed: true/false }` |
| POST | `/api/setup` | No | Create credentials `{ user, password }` → sets session cookie |
| POST | `/api/login` | No | Sign in `{ user, password }` → sets session cookie |
| POST | `/api/logout` | Yes | Clears session cookie |
| GET | `/api/tree` | Yes | Full directory tree as JSON |
| GET | `/api/file?path=...` | Yes | `{ filename, raw, html }` |
| POST | `/api/cache-dir?path=...` | Yes | Pre-render all files in a directory → `{ cached, skipped }` |

## Pages

| Path | Auth | Description |
|------|------|-------------|
| `/` | Yes | App (tree + MD viewer) or login page if unauthenticated |
| `/login` | No | Sign-in form (302 → `/` in setup mode) |

Setup page is served automatically at `/` when no config exists — no separate path.

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SIMD_DATA_DIR` | `/data` | Directory with documents to browse |
| `SIMD_CONFIG_DIR` | `/config` | Directory where `config.json` is stored |
| `SIMD_PORT` | `8080` | HTTP server port |
| `SIMD_SESSION_DURATION` | `86400` | Session lifetime in seconds (24h) |

## Stack

```
tiny_http       — synchronous HTTP, zero-dep
pulldown-cmark  — fast streaming Markdown parser
serde_json      — JSON serialization
bcrypt          — password hashing
```

No async runtime, no tokio, no tower. Synchronous I/O on dedicated threads.

## Project structure

```
simd/
├── src/
│   ├── main.rs       # HTTP loop, routing, response helpers
│   ├── config.rs     # Config read/write (config.json)
│   ├── auth.rs       # Login, session check, HTTP Basic Auth, setup flow
│   ├── session.rs    # In-memory session store (token + expiry + cleanup)
│   ├── cache.rs      # LRU file cache (100 entries) + TTL tree cache (30s)
│   ├── tree.rs       # Recursive directory tree builder → JSON
│   ├── markdown.rs   # Markdown → HTML, file serving, cache-dir pre-render
│   └── static/
│       ├── index.html  # Main SPA (CSS + JS embedded)
│       ├── login.html  # Sign-in form (B&W minimal)
│       └── setup.html  # First-run credentials setup (B&W minimal)
├── Dockerfile        # Multi-stage → scratch (~8MB)
└── Cargo.toml
```

## Performance

| Metric | Value |
|--------|-------|
| Binary size | ~950 KB (stripped) |
| Docker image | ~8 MB (scratch) |
| RSS memory | ~5–8 MB idle |
| Dependencies | 5 crates |
| Lines of code | ~1000 |

## Build

```bash
cargo build --release
```

The release profile optimises for size (`opt-level = "z"`, `lto = true`, `strip = true`).

## Docker

```bash
docker build -t simd .

docker run -d \
  --name simd \
  --restart unless-stopped \
  -p 8080:8080 \
  -v ~/documents:/data \
  -v simd-config:/config \
  simd
```

Config persists in a named Docker volume (`simd-config:/config`). Documents mounted separately (`~/documents:/data`).
