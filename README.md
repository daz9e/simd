# simd

> Read your md.

Minimal self-hosted Markdown viewer — single binary, ~4 MB, no runtime deps.

- Markdown rendering with GFM tables & task lists
- Tree sidebar, full-text search
- Password auth with bcrypt sessions
- Dark & light themes

## Run

```bash
docker run -d -p 8080:8080 -v ./notes:/data daz9e/simd:latest
```

Or build from source:

```bash
cargo build --release
SIMD_DATA_DIR=./notes SIMD_PORT=8080 ./target/release/simd
```

Open `http://localhost:8080` — create credentials on first visit.

## Config

| Variable | Default |
|---|---|
| `SIMD_DATA_DIR` | `/data` |
| `SIMD_CONFIG_DIR` | `/config` |
| `SIMD_PORT` | `8080` |
| `SIMD_SESSION_DURATION` | `86400` |
