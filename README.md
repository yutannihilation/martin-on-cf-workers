Martin on Cloudflare Workers
============================

This repository is an experiment to deploy Martin tile server to Cloudflare Workers.

## Why?

Why not? Now that [Cloudflare Workers supports tokio](https://blog.cloudflare.com/rust-workers-emscripten-target), it should be possible.

## Difficulties

- This still requires a forked version of tokio and mio just as [the official examples do](https://github.com/cloudflare/workers-rs/blob/1d668cfe1f2cc6bfe86232ed8abbf4b5e24522e3/examples/emscripten-tokio/Cargo.toml#L26-L31).
  - tokio: there are still some remaining pull requests.
  - mio: the pull request was merged, but not released on crates.io yet.
  - (libc: the pull request was merged and a new version was already released)
- I couldn't compile martin without disabling some features, which are not necessary; S3-related crates like aws-lc-sys are not necessary because workers-rs provides access to R2.
- wasm-streams is a bit tricky. Here's AI's explanation:
  
  > Why: wasm-streams 0.7.0 added `#[wasm_bindgen(private)]` to its internal stream adapter structs. In wasm-bindgen's Emscripten backend (`--target emscripten`), private structs are emitted without `${id}__force: true` and without `${id}` in the caller's `__deps`, causing Emscripten's JS linker to dead-code-eliminate the JS classes and fail at runtime with `ReferenceError: IntoUnderlyingSource is not defined`.

## License

- `webp2.pmtiles` is copied from <https://github.com/maplibre/martin/blob/main/tests/fixtures/pmtiles2/webp2.pmtiles>, which is licensed under the MIT license.
