//! Fuzz target: WAL frame parser (crash detector).
//!
//! INV-FERR-008: WAL recovery must handle corrupted/truncated/malformed
//! frames gracefully. Invalid frames are silently skipped; valid frames
//! are returned. A panic or OOM is a bug.

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Guard: reject inputs >64MB (WAL files can be large, but fuzzing
    // at that scale is not productive).
    if data.len() > 64 * 1024 * 1024 {
        return;
    }

    // Crash oracle: create a WAL file from arbitrary bytes and attempt recovery.
    // The recovery must not panic — it should return Ok (with valid frames)
    // or Err (corrupted). Either is acceptable.
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path().join("fuzz.wal");
    std::fs::write(&path, data).expect("write fuzz WAL file");

    if let Ok(mut wal) = ferratomic_db::wal::Wal::open(&path) {
        let _ = wal.recover();
    }
});
