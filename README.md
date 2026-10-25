Martin on Cloudflare Workers
============================

This repository is an experiment to deploy Martin tile server to Cloudflare Workers.

## Why?

Why not? Now that [Cloudflare Workers supports tokio](https://blog.cloudflare.com/rust-workers-emscripten-target), it should be possible.

## Difficulties

- This still requires a forked version of tokio and mio just as [the official examples do](https://github.com/cloudflare/workers-rs/blob/1d668cfe1f2cc6bfe86232ed8abbf4b5e24522e3/examples/emscripten-tokio/Cargo.toml#L26-L31).
  - tokio: there are still some remaining pull requests.
  - mio: [the pull request was merged](https://github.com/tokio-rs/mio/pull/1969), but not released on crates.io yet.
  - (libc: the pull request was merged and a new version was already released)
- I couldn't compile martin without disabling some features, which are not necessary; S3-related crates like aws-lc-sys are not necessary because workers-rs provides access to R2.
- wasm-streams hits a wasm-bindgen's bug, [which is already fixed in the upstream](https://github.com/wasm-bindgen/wasm-bindgen/pull/5415). We just need to wait until the next version (0.2.130) of wasm-bindgen is released.

## License

- `webp2.pmtiles` is copied from <https://github.com/maplibre/martin/blob/main/tests/fixtures/pmtiles2/webp2.pmtiles>, which is licensed under the MIT license.
