//! Choosing and binding the pdfium shared library.

use crate::InitError;
use pdfium_render::prelude as pr;
use std::path::PathBuf;

/// The pdfium shared library a process binds.
///
/// Its default is [`Library::Bundled`] with the `bundled` feature and [`Library::System`]
/// without it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Library {
    /// The pdfium library embedded at build time.
    ///
    /// Binding writes it to a new directory under [`std::env::temp_dir`] and loads it from there.
    /// The file is deleted once loaded, except on Windows, which cannot delete a loaded DLL.
    #[cfg(feature = "bundled")]
    #[default]
    Bundled,
    /// The pdfium shared library file at this path.
    ///
    /// Loading a library runs its code, so name only a pdfium build you trust.
    /// It must provide every function the pinned `pdfium-render` binds, as the pdfium release
    /// the README names does.
    Path(PathBuf),
    /// The pdfium library found through the platform's library search path.
    #[cfg_attr(not(feature = "bundled"), default)]
    System,
}

/// Loads `library` and initializes pdfium with it.
///
/// pdfium-render keeps the bindings in a process-wide cell, so this must run at most once.
pub(crate) fn bind(library: &Library) -> Result<(), InitError> {
    match library {
        #[cfg(feature = "bundled")]
        Library::Bundled => bundled::bind(),
        Library::Path(path) => initialize(pr::Pdfium::bind_to_library(path)),
        Library::System => initialize(pr::Pdfium::bind_to_system_library()),
    }
}

/// Initializes pdfium with freshly loaded bindings.
///
/// `Pdfium::new` calls `FPDF_InitLibrary` and installs the bindings in pdfium-render's
/// process-wide cell, where the `pr::Pdfium::default()` calls in `safe.rs` find them.
fn initialize(
    bindings: Result<Box<dyn pr::PdfiumLibraryBindings>, pr::PdfiumError>,
) -> Result<(), InitError> {
    pr::Pdfium::new(bindings.map_err(InitError::BindLibrary)?);
    Ok(())
}

#[cfg(feature = "bundled")]
mod bundled {
    use super::initialize;
    use crate::InitError;
    use pdfium_render::prelude as pr;

    /// The library build.rs downloaded for the target, or an empty file on docs.rs.
    const EMBEDDED_LIBRARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pdfium"));

    /// Writes the embedded library to a temporary directory and binds it from there.
    pub(super) fn bind() -> Result<(), InitError> {
        let mut builder = tempfile::Builder::new();
        builder.prefix("pdfium-");
        // Only this user may write to the directory, so nobody can swap the library between
        // writing and loading it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let directory = builder
            .tempdir()
            .map_err(|source| InitError::WriteBundledLibrary {
                directory: std::env::temp_dir(),
                source,
            })?;
        let path = directory
            .path()
            .join(pr::Pdfium::pdfium_platform_library_name());
        std::fs::write(&path, EMBEDDED_LIBRARY).map_err(|source| {
            InitError::WriteBundledLibrary {
                directory: directory.path().to_path_buf(),
                source,
            }
        })?;

        let result = initialize(pr::Pdfium::bind_to_library(&path));
        // A loaded library stays mapped after its file is deleted on Linux and macOS.
        // Windows refuses to delete a loaded DLL, and `TempDir` ignores that failure, so the
        // directory stays behind there.
        drop(directory);
        result
    }
}
