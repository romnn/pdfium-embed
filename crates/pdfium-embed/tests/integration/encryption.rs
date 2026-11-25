//! Opening encrypted documents, with and without their password.
//!
//! Both fixtures use the standard security handler (revision 3, 128-bit RC4) and have one empty
//! page, so they hold no encrypted content and stay plain text.

use color_eyre::eyre;
use googletest::prelude::*;
use pdfium_embed::{Document, DocumentError, Pdfium};

/// Opens only with the user password `user` or the owner password `owner`.
fn user_password_pdf() -> eyre::Result<Vec<u8>> {
    crate::fixture("encrypted-user-password.pdf")
}

/// Has an empty user password and the owner password `owner`, and forbids printing, changing
/// and copying.
fn empty_password_pdf() -> eyre::Result<Vec<u8>> {
    crate::fixture("encrypted-empty-password.pdf")
}

/// Opens a document and returns whether it is encrypted.
fn is_encrypted(
    pdfium: &Pdfium,
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<bool, DocumentError> {
    let is_encrypted = |document: &Document<'_>| Ok(document.is_encrypted());
    match password {
        Some(password) => pdfium.with_document_password(pdf_bytes, password, is_encrypted),
        None => pdfium.with_document(pdf_bytes, is_encrypted),
    }
}

#[test]
fn password_protected_document_needs_its_password() -> eyre::Result<()> {
    let pdf_bytes = user_password_pdf()?;
    let pdfium = Pdfium::instance()?;

    // Without a password, or with a wrong one, opening reports the encryption.
    for password in [None, Some("wrong")] {
        assert_that!(
            is_encrypted(pdfium, &pdf_bytes, password),
            err(matches_pattern!(DocumentError::Encrypted(_))),
            "{password:?}"
        );
    }
    assert_that!(
        pdfium.num_pages(&pdf_bytes),
        err(matches_pattern!(DocumentError::Encrypted(_)))
    );

    // Either password opens it, and the open document still says it is encrypted.
    for password in ["user", "owner"] {
        assert_that!(
            is_encrypted(pdfium, &pdf_bytes, Some(password)),
            ok(eq(&true)),
            "{password}"
        );
    }
    Ok(())
}

/// A document with an empty user password opens like any other, so only `is_encrypted` tells
/// the caller it is encrypted.
#[test]
fn empty_password_document_opens_but_is_encrypted() -> eyre::Result<()> {
    let pdf_bytes = empty_password_pdf()?;
    let pdfium = Pdfium::instance()?;

    assert_that!(is_encrypted(pdfium, &pdf_bytes, None), ok(eq(&true)));
    assert_that!(pdfium.num_pages(&pdf_bytes)?, eq(1));
    Ok(())
}

#[test]
fn unencrypted_document_is_not_encrypted() -> eyre::Result<()> {
    let pdf_bytes = crate::fixture("text.pdf")?;
    let pdfium = Pdfium::instance()?;

    // A password is ignored for a document that has none.
    assert_that!(is_encrypted(pdfium, &pdf_bytes, None), ok(eq(&false)));
    assert_that!(
        is_encrypted(pdfium, &pdf_bytes, Some("unused")),
        ok(eq(&false))
    );
    Ok(())
}
