//! Embeds the prebuilt pdfium library for the target when the `bundled` feature is on.

#[cfg(feature = "bundled")]
fn main() -> eyre::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");
    bundled::embed()
}

/// Without `bundled`, the program binds a library it provides at run time, so there is nothing
/// to build.
#[cfg(not(feature = "bundled"))]
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
}

#[cfg(feature = "bundled")]
mod bundled {
    use eyre::WrapErr as _;
    use sha2::Digest as _;
    use std::io::Read as _;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// The pdfium-binaries release whose ABI the pinned `pdfium-render` binds.
    ///
    /// It must match the release that `pdfium-render`'s `pdfium_latest` feature names; see the
    /// comment on that dependency in Cargo.toml.
    const TAG: &str = "chromium/7881";

    const DOWNLOAD_ATTEMPTS: u32 = 4;
    const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(2);
    const RETRY_DELAY: Duration = Duration::from_secs(5);

    /// Bounds the archive download; the largest archive embedded here is about 7 MiB.
    const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

    /// A prebuilt pdfium archive from pdfium-binaries.
    struct Asset {
        name: &'static str,
        /// SHA-256 of the archive, as the release publishes it.
        sha256: &'static str,
        /// Path of the shared library inside the archive.
        member: &'static str,
    }

    /// Writes the pdfium library to embed to `$OUT_DIR/pdfium`, where `src/library.rs` includes it.
    pub fn embed() -> eyre::Result<()> {
        println!("cargo::rerun-if-env-changed=PDFIUM_LIB_DIR");
        println!("cargo::rerun-if-env-changed=DOCS_RS");
        let output =
            PathBuf::from(std::env::var("OUT_DIR").wrap_err("OUT_DIR is not set")?).join("pdfium");

        // docs.rs builds without network access, and documentation does not need the library.
        if std::env::var_os("DOCS_RS").is_some() {
            return std::fs::write(&output, b"").wrap_err("write an empty library for docs.rs");
        }

        let target_os = std::env::var("CARGO_CFG_TARGET_OS")?;
        let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH")?;
        let target_env = std::env::var("CARGO_CFG_TARGET_ENV")?;

        // Offline build: embed a library the user provides instead of downloading one.
        if let Some(lib_dir) = std::env::var_os("PDFIUM_LIB_DIR") {
            let library = Path::new(&lib_dir).join(library_file_name(&target_os));
            println!("cargo::rerun-if-changed={}", library.display());
            std::fs::copy(&library, &output)
                .wrap_err_with(|| format!("copy {} named by PDFIUM_LIB_DIR", library.display()))?;
            return Ok(());
        }

        let Some(asset) = asset(&target_os, &target_arch, &target_env) else {
            eyre::bail!(
                "pdfium-binaries has no prebuilt pdfium for {target_arch}-{target_os}-{target_env}; \
                 turn off the default `bundled` feature of pdfium-embed and bind a pdfium library \
                 at run time with `Pdfium::bind`"
            );
        };
        let url = format!(
            "https://github.com/bblanchon/pdfium-binaries/releases/download/{}/{}",
            TAG.replace('/', "%2F"),
            asset.name
        );
        let archive = download(&url).wrap_err_with(|| {
            format!(
                "download pdfium from {url}; to build offline, set PDFIUM_LIB_DIR to a directory \
                 that holds {} from that archive",
                asset.member
            )
        })?;

        let digest = format!("{:x}", sha2::Sha256::digest(&archive));
        if digest != asset.sha256 {
            eyre::bail!(
                "{} has SHA-256 {digest}, but {TAG} published {}",
                asset.name,
                asset.sha256
            );
        }

        let library = extract(&archive, asset.member)
            .wrap_err_with(|| format!("extract {} from {}", asset.member, asset.name))?;
        std::fs::write(&output, library).wrap_err("write the pdfium library")
    }

    /// Returns the archive for a target, or `None` when pdfium-binaries publishes none.
    fn asset(target_os: &str, target_arch: &str, target_env: &str) -> Option<Asset> {
        let (name, sha256) = match (target_os, target_arch, target_env) {
            ("linux", "x86_64", "gnu") => (
                "pdfium-linux-x64.tgz",
                "1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d",
            ),
            ("linux", "aarch64", "gnu") => (
                "pdfium-linux-arm64.tgz",
                "ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674",
            ),
            ("linux", "x86_64", "musl") => (
                "pdfium-linux-musl-x64.tgz",
                "8a8cdfc6c79865269f74bf0e9ca476cdd8a9908ea7e0b0abcfc1525d59fb7257",
            ),
            ("linux", "aarch64", "musl") => (
                "pdfium-linux-musl-arm64.tgz",
                "4c6e678e820c390c3ffc1f720a25386e3b637aa42ecf99446672146d793dac46",
            ),
            ("macos", "x86_64", _) => (
                "pdfium-mac-x64.tgz",
                "6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b",
            ),
            ("macos", "aarch64", _) => (
                "pdfium-mac-arm64.tgz",
                "52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40",
            ),
            ("windows", "x86_64", _) => (
                "pdfium-win-x64.tgz",
                "73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08",
            ),
            ("windows", "aarch64", _) => (
                "pdfium-win-arm64.tgz",
                "d3035d4d2cacac6ecd1a2ece197a3d702a1b2a58466276b9f870b8cb278a9d84",
            ),
            _ => return None,
        };
        // Windows archives keep the DLL in `bin/`; `lib/` holds its import library.
        let member = match target_os {
            "windows" => "bin/pdfium.dll",
            "macos" => "lib/libpdfium.dylib",
            _ => "lib/libpdfium.so",
        };
        Some(Asset {
            name,
            sha256,
            member,
        })
    }

    /// Returns the file name of the pdfium library on a target OS.
    fn library_file_name(target_os: &str) -> &'static str {
        match target_os {
            "windows" => "pdfium.dll",
            "macos" => "libpdfium.dylib",
            _ => "libpdfium.so",
        }
    }

    /// Downloads `url`, retrying a failed or stalled attempt a few times to ride out brief
    /// network failures.
    fn download(url: &str) -> eyre::Result<Vec<u8>> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .build()
            .into();
        let mut attempt = 1;
        loop {
            let result = agent.get(url).call().and_then(|mut response| {
                response
                    .body_mut()
                    .with_config()
                    .limit(MAX_ARCHIVE_BYTES)
                    .read_to_vec()
            });
            match result {
                Ok(archive) => return Ok(archive),
                Err(err) if attempt < DOWNLOAD_ATTEMPTS => {
                    println!("cargo::warning=download of {url} failed ({err}), retrying");
                    std::thread::sleep(RETRY_DELAY);
                    attempt += 1;
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// Returns the contents of `member` from a gzipped tar archive.
    fn extract(archive: &[u8], member: &str) -> eyre::Result<Vec<u8>> {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        for entry in archive.entries()? {
            let mut entry = entry?;
            if entry.path()? == Path::new(member) {
                let mut library = Vec::new();
                entry.read_to_end(&mut library)?;
                return Ok(library);
            }
        }
        eyre::bail!("the archive has no {member}")
    }
}
