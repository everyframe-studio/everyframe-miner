use std::{fs, path::Path};

/// macOS exposes TMPDIR through /var -> /private/var. Resolve only the
/// test-harness-owned temporary root, not user-supplied credential paths.
pub fn tempdir_in(root: &Path) -> tempfile::TempDir {
    let physical_root = fs::canonicalize(root).expect("resolve test temporary root");
    tempfile::tempdir_in(physical_root).expect("create private test directory")
}

pub fn tempdir() -> tempfile::TempDir {
    tempdir_in(&std::env::temp_dir())
}
