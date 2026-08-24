# pstream-resolver-rs

The stream resolver behind [pstream.watch](https://pstream.watch). It runs on
a phone.

Given a TMDB id it asks thirteen providers in parallel for a playable stream,
returns every one that answered, and proxies the playback traffic for the
sources whose CDNs refuse a browser directly.

It ships as **one statically linked aarch64 binary, about 3.4 MB**. The device
needs no Node runtime and no `node_modules` — the previous deployment pulled
506 npm packages to use four of them.

## Layout

```
crates/shared     the parts the backend also needs
  extractors/     13 providers; 9 are a table, 4 have real logic
  http.rs         shared clients: direct, residential proxy, TLS-permissive
  proxy.rs        /proxy/stream and the m3u8 rewriter
  probe.rs        /api/media-probe, with a guard against local targets
  cache.rs        6h TTL, LRU-ish eviction
  cors.rs         origin allowlist
  ratelimit.rs    30 provider-hitting requests per IP per minute
  health.rs       per-provider success tracking

crates/resolver   the binary: routing and request validation
```

## Endpoints

| | |
|---|---|
| `GET /` · `/api/ping` | liveness |
| `GET /api/stream` | resolve; `tmdbId`, `type`, `season`, `episode`, `title`, `year` |
| `GET /proxy/stream` | byte relay, rewriting m3u8 so segments come back through here |
| `GET /api/media-probe` | first 2 MB of a URL, for reading MKV/MP4 headers |
| `GET /api/youtube/search` | trailer search, scraped — no API key to leak |
| `GET /api/subtitles/subdl` | subtitle search; needs `SUBDL_API_KEY` |

## Building

Cross-compiling needs a C compiler for the target, because rustls' crypto
backend has native code in it. This uses **zig**, which carries musl headers
for every target it supports — so there is no Android NDK and no musl-cross
toolchain to install.

```sh
rustup target add aarch64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl
```

`.cargo/config.toml` points `CC` at a zig wrapper and links with `rust-lld`.
Targeting musl rather than `aarch64-linux-android` is deliberate: a fully
static ELF only makes syscalls, which Android's kernel serves like any other
Linux, and it removes the NDK from the build entirely.

```sh
cargo test --workspace
```

## Deployment

CI builds the binary on every push to `main` and publishes it to the rolling
`latest` release. An updater on the device polls the published checksum every
five minutes and swaps the binary in when it changes.

The swap is write-then-rename. Writing over the file directly fails with
`ETXTBSY` while it is executing, and the supervisor restarts it within two
seconds — `rename()` puts a new inode in place instead, which the running
process is unaffected by.

Three supervised loops run on the device: the resolver, the Cloudflare tunnel,
and the updater. Nothing on Android restarts a crashed background process, so
without them a single crash took the service down until the next reboot.

### If the tunnel won't connect

Cloudflare error 1033 means `cloudflared` has not registered. It defaults to
QUIC on UDP 7844, which many public networks drop; `--protocol http2` moves it
to TCP and usually fixes it. A network that blocks port 7844 on *both*
protocols cannot be worked around at all — that needs a different connection.

## Configuration

| variable | effect if unset |
|---|---|
| `PORT` | 8790 |
| `SUBDL_API_KEY` | subtitle search reports itself unconfigured |
| `RESIDENTIAL_PROXY_URL` | providers are reached directly |
| `DEPLOY_SECRET` | `/api/deploy` answers 404 rather than advertising itself |

## License

MIT — see [LICENSE](LICENSE).
