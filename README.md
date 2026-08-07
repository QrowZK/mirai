# mirai
mirai no tame ni.

An extremely lightweight, privacy-focused web browser built on the
[Servo](https://servo.org) engine.

## Privacy by default

- **Ad & tracker blocking built into the engine layer.** Every HTTP request
  Servo makes passes through Mirai's interception hook, where it is checked
  against bundled EasyList + EasyPrivacy filter lists (via Brave's
  [`adblock`](https://github.com/brave/adblock-rust) engine). Blocked requests
  never leave the machine.
- **No telemetry.** Neither Servo nor Mirai phones home. The filter lists are
  vendored into the binary, so even the first launch makes no housekeeping
  network requests.

## Project layout

| Path | Purpose |
|---|---|
| `crates/mirai` | The browser: winit window, Servo webview glue, delegates |
| `crates/mirai-privacy` | Servo-free privacy engine (filter-list blocking); fast to build and unit test |
| `assets/filter-lists` | Vendored EasyList / EasyPrivacy snapshots |

## Building

Requires the Rust toolchain pinned in `rust-toolchain.toml` plus Servo's
[platform build dependencies](https://book.servo.org/hacking/setting-up-your-environment.html).
Servo is compiled from source (pinned git revision), so the first build is
long and needs several GB of disk.

```sh
cargo run -p mirai [url]
```

The privacy engine alone builds and tests in minutes:

```sh
cargo test -p mirai-privacy
```

Windows is the primary release target; binaries are produced by the
`build-windows` CI job.

## Media playback

Video/audio support is feature-gated behind `--features media` and uses
GStreamer. Build with the GStreamer development libraries installed
(`libgstreamer1.0-dev` + base/good/bad plugin dev packages on Debian/Ubuntu,
the official GStreamer MSVC runtime + development installers on Windows).

## Status

Working: toolbar, URL bar with search fallback, tabs, keyboard shortcuts,
ad/tracker blocking with per-tab counters and a settings toggle, compiled
filter-list cache for fast startup, Windows installer pipeline
(tag `vX.X.X` or dispatch the Release workflow). Media playback is
best-effort while Servo's GStreamer backend matures.
