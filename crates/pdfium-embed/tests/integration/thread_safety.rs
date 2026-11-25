//! Concurrent use of the process-wide instance.

use color_eyre::eyre;
use googletest::prelude::*;
use pdfium_embed::Pdfium;
use std::sync::Arc;

/// Concurrent access through the safe [`Pdfium`] API is fully serialised by
/// the internal mutex and never produces spurious failures.
#[test]
fn concurrent_access_is_safe() -> eyre::Result<()> {
    let pdf_bytes = crate::fixture("origin-rotate-0.pdf")?;
    let pdf_bytes = Arc::new(pdf_bytes);

    let num_threads = 8;
    let iterations_per_thread = 20;
    let barrier = Arc::new(std::sync::Barrier::new(num_threads));

    let pdfium = Pdfium::instance()?;
    let mut handles = Vec::new();
    for _ in 0..num_threads {
        let pdf_bytes = Arc::clone(&pdf_bytes);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let mut failures = 0_u32;
            for _ in 0..iterations_per_thread {
                if pdfium.num_pages(&pdf_bytes).is_err() {
                    failures += 1;
                }
            }
            failures
        }));
    }

    let total_failures: u32 = handles
        .into_iter()
        .map(|handle| handle.join().unwrap_or(u32::MAX))
        .sum();

    assert_that!(total_failures, eq(0));
    Ok(())
}
