# pdfium-embed

[![crates.io](https://img.shields.io/crates/v/pdfium-embed.svg)](https://crates.io/crates/pdfium-embed)
[![docs.rs](https://img.shields.io/docsrs/pdfium-embed)](https://docs.rs/pdfium-embed)
[![test](https://github.com/romnn/pdfium-embed/actions/workflows/test.yaml/badge.svg)](https://github.com/romnn/pdfium-embed/actions/workflows/test.yaml)

Safe, serialized access to [pdfium] from Rust, with the pdfium library embedded in your binary
at build time.

Your program needs no pdfium installed, downloads nothing at run time, and keeps no cache: the
build script downloads the prebuilt pdfium for the target from [pdfium-binaries], checks it
against the SHA-256 the release publishes, and embeds it.
At run time, the first call writes the library to a temporary directory and loads it from there.

## How it differs

- **From [pdfium-render]**, which this crate wraps: pdfium-render loads a pdfium you ship and
  locate yourself, and its `thread_safe` feature only marks its types `Send` and `Sync`.
  pdfium is not thread-safe, so pdfium-embed holds one process-wide lock around every call
  into it and never hands out pdfium-render's types.
- **From [pdfium-auto] and [pdfium-bundled]**: both fetch pdfium for you, into a per-user cache
  at run time or embedded at build time behind a `bundled` feature, and return pdfium-render's
  own types.
  pdfium-embed has no run-time download path at all: the build embeds the library, or, with the
  `bundled` feature off, you bring your own.
  It also pins pdfium-render exactly (`=0.9.3`) to the one pdfium release its `pdfium_latest`
  feature binds (`chromium/7881`), so the embedded library's ABI always matches the bindings.

The API is small: page count, page size, rendering to an RGBA image with a mandatory bound on
the longest pixel edge, plain text, whole or within a character limit checked before the text is
copied out of pdfium, positioned text segments and words, the visible page frame with its
rotation, whether a document is encrypted, and copying a page range into a new PDF.

## Usage

```bash
cargo add pdfium-embed
```

```rust
use pdfium_embed::{DEFAULT_MAX_RENDER_DIMENSION, Pdfium};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A two-page PDF from this repository's tests.
    let pdf_bytes = std::fs::read("tests/fixtures/text.pdf")?;
    let pdfium = Pdfium::instance()?;

    let page_count = pdfium.num_pages(&pdf_bytes)?;
    let page = pdfium.render_page(&pdf_bytes, 0, 150, DEFAULT_MAX_RENDER_DIMENSION)?;
    let text = pdfium.page_text(&pdf_bytes, 1)?;
    println!(
        "{page_count} pages; the first renders to {}x{} pixels; the second reads {text:?}",
        page.image.width(),
        page.image.height(),
    );

    // `with_document` loads the PDF once for several calls.
    let words = pdfium.with_document(&pdf_bytes, |document| document.page_text_words(0))?;
    assert!(words.iter().any(|word| word.text == "fox"));
    Ok(())
}
```

`Pdfium::instance` binds the library on its first successful call and returns the same instance
from then on.
Every method takes the process-wide lock, so the instance can be shared across threads and
async tasks.

## Encrypted documents

Opening a PDF that needs a password fails with `DocumentError::Encrypted`, and
`Pdfium::with_document_password` opens it with its user or owner password.
A PDF with an empty user password opens without one, so `Document::is_encrypted` tells whether
an open document is encrypted all the same.

```rust
use pdfium_embed::{DocumentError, Pdfium};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // An empty page from this repository's tests, encrypted with the user password "user".
    let pdf_bytes = std::fs::read("tests/fixtures/encrypted-user-password.pdf")?;
    let pdfium = Pdfium::instance()?;

    assert!(matches!(
        pdfium.num_pages(&pdf_bytes),
        Err(DocumentError::Encrypted(_))
    ));
    let encrypted = pdfium.with_document_password(&pdf_bytes, "user", |document| {
        Ok::<_, DocumentError>(document.is_encrypted())
    })?;
    assert!(encrypted);
    Ok(())
}
```

## Features

| Feature             | Effect                                                                                    |
| ------------------- | ----------------------------------------------------------------------------------------- |
| `bundled` (default) | The build script downloads pdfium for the target and embeds it; `Library::Bundled` binds it. |

With `bundled` off, the build downloads nothing and the crate binds a pdfium library you provide:
the one on the system's library search path by default (`Library::System`), or a file you name.

```rust,no_run
use pdfium_embed::{Library, Pdfium};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Marked `no_run`, since this path exists only where you installed pdfium.
    let pdfium = Pdfium::bind(&Library::Path("/opt/pdfium/lib/libpdfium.so".into()))?;
    println!("{} pages", pdfium.num_pages(&std::fs::read("document.pdf")?)?);
    Ok(())
}
```

Call `Pdfium::bind` before the first `Pdfium::instance`; a process binds one library, once, and
a failed attempt binds nothing.
Loading a library runs its code, so name only a pdfium build you trust.
The library must provide every function pdfium-render 0.9.3 binds, as the `chromium/7881` release
of pdfium-binaries does.

## Supported targets

`bundled` embeds a pdfium-binaries build for these targets:

| Target                                                  | Archive                       |
| ------------------------------------------------------- | ----------------------------- |
| `x86_64-unknown-linux-gnu`                              | `pdfium-linux-x64.tgz`        |
| `aarch64-unknown-linux-gnu`                             | `pdfium-linux-arm64.tgz`      |
| `x86_64-unknown-linux-musl`                             | `pdfium-linux-musl-x64.tgz`   |
| `aarch64-unknown-linux-musl`                            | `pdfium-linux-musl-arm64.tgz` |
| `x86_64-apple-darwin`                                   | `pdfium-mac-x64.tgz`          |
| `aarch64-apple-darwin`                                  | `pdfium-mac-arm64.tgz`        |
| `x86_64-pc-windows-msvc`, `x86_64-pc-windows-gnu`       | `pdfium-win-x64.tgz`          |
| `aarch64-pc-windows-msvc`, `aarch64-pc-windows-gnullvm` | `pdfium-win-arm64.tgz`        |

CI runs the tests on Linux (x86-64), macOS (Apple silicon) and Windows (x86-64).
The macOS libraries need macOS 12 or later.

On musl, the pdfium library is a shared object, and a statically linked musl binary cannot load
one: build with `RUSTFLAGS="-C target-feature=-crt-static"` so the binary links musl dynamically.

On any other target, the build fails with an error that tells you to turn off the `bundled`
feature and bind a library at run time instead, unless `PDFIUM_LIB_DIR` names a library to embed.

## Offline builds

Set `PDFIUM_LIB_DIR` to a directory that holds the library, and the build embeds it instead of
downloading one:

```bash
PDFIUM_LIB_DIR=/path/to/pdfium/lib cargo build
```

The directory must contain `libpdfium.so`, `libpdfium.dylib` or `pdfium.dll` for the target,
such as the `lib/` directory of the release archive (`bin/` on Windows).
The build embeds that file as it is, without the SHA-256 check, so it should come from the
`chromium/7881` release.

## At run time

The first binding writes the embedded library to a new directory under the system's temporary
directory, which `TMPDIR` overrides on Unix, and loads it from there.
That directory must allow executing code, which a `/tmp` mounted `noexec` does not.
On Linux and macOS, the file is deleted as soon as it is loaded.
Windows cannot delete a loaded DLL, so there the directory stays behind after the process exits.

## License

pdfium-embed is licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT license](LICENSE-MIT), at your option.

With the `bundled` feature, your binary contains pdfium, so distributing it means following
pdfium's licenses as well.
pdfium is licensed under the BSD-3-Clause license, with parts under the Apache License 2.0.
The pdfium-binaries build also includes third-party libraries under their own licenses, among
them FreeType, ICU, libjpeg-turbo, libpng, LittleCMS, OpenJPEG, libtiff, zlib and Abseil, and the
build scripts themselves are MIT-licensed.
The release archive for your target carries every notice, in `LICENSE` and the `licenses/`
directory.

[pdfium]: https://pdfium.googlesource.com/pdfium/
[pdfium-binaries]: https://github.com/bblanchon/pdfium-binaries
[pdfium-render]: https://crates.io/crates/pdfium-render
[pdfium-auto]: https://crates.io/crates/pdfium-auto
[pdfium-bundled]: https://crates.io/crates/pdfium-bundled
