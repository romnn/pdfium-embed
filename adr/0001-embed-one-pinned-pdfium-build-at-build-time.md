# 1. Embed one pinned pdfium build at build time, never download at run time

**Status:** accepted · **Date:** 2026-11-25

## Context

pdfium-render loads pdfium dynamically and looks up every function it binds by name.
A pdfium older than the release its `pdfium_latest` feature names lacks newer entry points, such
as `FPDF_StructElement_GetExpansion`, and binding fails; a newer one may rename or drop a function
pdfium-render still calls.
Programs that use pdfium-render therefore have to ship a matching library and find it at run time.

Other crates solve this by downloading pdfium into a per-user cache at run time.
That puts an HTTP client, a cache layout and a network dependency into every program, and the
library a program loads depends on what the cache holds.

## Decision

- `pdfium-render` is pinned exactly in `crates/pdfium-embed/Cargo.toml`, and `TAG` in `build.rs`
  names the pdfium-binaries release its `pdfium_latest` feature binds.
  The two move together.
- With the default `bundled` feature, `build.rs` downloads that release's archive for the target,
  checks it against the SHA-256 the release publishes, and embeds the library in the crate.
  At run time the crate writes the library to a temporary directory, binds it, and deletes the
  file where the platform allows.
- `PDFIUM_LIB_DIR` replaces the download with a library the user provides, for offline builds.
  It is embedded unchecked.
- A target without a published archive fails the build with an error that names the way out:
  turning off `bundled`.
- With `bundled` off, the build downloads nothing, and the program binds a library it names at
  run time through `Library::Path`, or the system's through `Library::System`.
- There is no run-time download and no cache.

## Consequences

- Programs carry about 6 MiB more, and a cross-compiled build needs network access or
  `PDFIUM_LIB_DIR`.
- Bumping pdfium-render means moving `TAG` and every digest in `build.rs` to the release its new
  `pdfium_latest` names.
- Targets pdfium-binaries does not publish for are supported only with `bundled` off.
- A `/tmp` mounted `noexec` stops the embedded library from loading; `TMPDIR` moves it.
- On Windows the extracted DLL cannot be deleted while loaded, so each process leaves a copy in
  the temporary directory.

## References

- `crates/pdfium-embed/build.rs`: the release tag, the per-target archives and digests, and the
  download.
- `crates/pdfium-embed/src/library.rs`: `Library` and the binding of the embedded library.
- [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) releases.
