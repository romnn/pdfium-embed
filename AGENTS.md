# Agent guidelines

`pdfium-embed` gives Rust programs safe, serialized access to pdfium, with the pdfium library
embedded at build time.
Read [README.md](README.md) for what the crate does and the decision records in [adr/](adr) before
changing an area one governs.
A change that alters a recorded decision needs a new ADR that supersedes the old one.

## Layout

- `crates/pdfium-embed/build.rs`: downloads and checks the pdfium library for the target.
- `crates/pdfium-embed/src/library.rs`: the library choice and binding.
- `crates/pdfium-embed/src/safe.rs`: the serialized API.
- `crates/pdfium-embed/tests/fixtures`: small, uncompressed PDFs the tests read.

## Building and testing

- Use the `task` commands in `taskfile.yaml`: `task check`, `task test`, `task lint`, and before
  calling work done `task lint:fc`, `task test:fc`, `task test:doc` and `task spellcheck`.
- The first build downloads pdfium; `PDFIUM_LIB_DIR` points at a local copy instead.
- Bumping `pdfium-render` means moving `TAG` and every digest in `build.rs` with it
  ([ADR 1](adr/0001-embed-one-pinned-pdfium-build-at-build-time.md)).
