# 2. Serialize every pdfium call behind one process-wide lock

**Status:** accepted · **Date:** 2026-11-25

## Context

pdfium is not thread-safe: its parser state and error codes are process-global and unprotected.
pdfium-render's `thread_safe` feature only marks its wrapper types `Send` and `Sync`; it does not
serialize calls into pdfium.
Handing pdfium-render's types to callers would leave every caller responsible for locking, and
one unlocked call races with all the others.

pdfium-render also keeps its bindings in a process-wide cell that can be set once, so a process
can bind only one library.

## Decision

- `Pdfium` is a process-wide singleton, created by the first successful `Pdfium::instance` or
  `Pdfium::bind` call; a process binds one `Library`, once, and a failed attempt binds nothing.
- Every `Pdfium` method takes one process-wide mutex for its whole duration, including the
  closure `Pdfium::with_document` runs.
- `Document`, the handle that closure receives, is not `Sync`, so threads the closure spawns
  cannot call into pdfium through it.
- The public API exposes only this crate's own types; pdfium-render's types appear only as the
  `source` of errors.

## Consequences

- pdfium work never runs in parallel within a process; programs that need parallel rendering run
  several processes.
- Every new operation is a `Pdfium` or `Document` method, so it inherits the lock.
- Another crate in the same process that calls pdfium-render directly bypasses the lock.

## References

- `crates/pdfium-embed/src/safe.rs`: `Pdfium`, `Document` and the lock.
