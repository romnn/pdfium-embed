//! Choosing the library a process binds.

#[cfg(feature = "bundled")]
use color_eyre::eyre;
use pdfium_embed::{InitError, Library, Pdfium};

/// The default library is the embedded one, and the first binding is final for the process.
#[cfg(feature = "bundled")]
#[test]
fn instance_binds_the_embedded_library_once() -> eyre::Result<()> {
    let instance = Pdfium::instance()?;

    // Naming the bound library again returns the same instance.
    assert!(std::ptr::eq(instance, Pdfium::bind(&Library::Bundled)?));

    // Another library cannot replace it.
    let Err(err) = Pdfium::bind(&Library::System) else {
        eyre::bail!("binding a second library succeeded");
    };
    assert!(matches!(err, InitError::AlreadyBound), "{err:?}");
    Ok(())
}

/// A library path that does not exist fails to bind, and the failure binds nothing, so a later
/// call can still bind another library.
#[cfg(not(feature = "bundled"))]
#[test]
fn a_failed_binding_leaves_the_process_unbound() {
    for path in ["missing/libpdfium.so", "also-missing/libpdfium.so"] {
        let err = Pdfium::bind(&Library::Path(path.into())).err();
        // The second attempt loads its own library rather than reporting `AlreadyBound`.
        assert!(matches!(err, Some(InitError::BindLibrary(_))), "{err:?}");
    }
}
