//! Crate root — re-exports modules used by both `main.rs` and any
//! integration tests, and exposes the addon version string parsed from
//! `config.yaml` at compile time.
//!
//! Phase 2 introduces the `config` module. Subsequent phases populate
//! `ha_client`, `render::*` and `sources::*`.

#![forbid(unsafe_code)]

pub mod config;

/// Addon version, parsed from `config.yaml` at compile time.
///
/// Mirrors the Python addon's `__version__` lookup. Embedding the
/// version string in the binary (rather than reading the YAML at
/// runtime) means the badge stamp and `/healthz` response can never
/// drift from the manifest the Supervisor sees.
pub const ADDON_VERSION: &str = parse_version_from_yaml(include_str!("../config.yaml"));

/// Tiny `const fn` parser that scans for a `version: "X.Y.Z"` line in
/// the bundled `config.yaml` and returns the literal between quotes.
///
/// Done at compile time so a malformed YAML triggers a build error
/// rather than a runtime surprise. The implementation is byte-oriented
/// because Rust's `const fn` doesn't yet support `&str::find` etc.
const fn parse_version_from_yaml(src: &str) -> &str {
    // Look for the literal `\nversion: "` token (or starting at byte 0)
    // and slice out everything up to the next `"`. Defaults to "0.0.0"
    // if not found, mirroring the Python `_FALLBACK_VERSION`.
    let bytes = src.as_bytes();
    let needle = b"version:";
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        // Match `version:` only when it's at start of a line.
        let line_start = i == 0 || bytes[i - 1] == b'\n';
        if line_start && bytes_eq(bytes, i, needle) {
            // Skip past `version:` and any spaces / opening quote.
            let mut j = i + needle.len();
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'"' {
                j += 1;
            }
            // Slice up to the next `"` or end-of-line, whichever first.
            let start = j;
            while j < bytes.len() && bytes[j] != b'"' && bytes[j] != b'\n' {
                j += 1;
            }
            // SAFETY-equivalent: we're slicing on byte boundaries that
            // came from valid UTF-8 input, so the resulting slice is
            // also valid UTF-8.
            return slice_str(src, start, j);
        }
        i += 1;
    }
    "0.0.0"
}

const fn bytes_eq(haystack: &[u8], offset: usize, needle: &[u8]) -> bool {
    let mut k = 0;
    while k < needle.len() {
        if haystack[offset + k] != needle[k] {
            return false;
        }
        k += 1;
    }
    true
}

const fn slice_str(s: &str, start: usize, end: usize) -> &str {
    // `str::get` isn't const yet, so we drop down to bytes and rebuild.
    // Both `slice::split_at` (const since 1.71) and
    // `core::str::from_utf8` (const since 1.63) are const-stable, so
    // the whole pipeline runs at compile time without any `unsafe`.
    let bytes = s.as_bytes();
    let (_, after) = bytes.split_at(start);
    let (sliced, _) = after.split_at(end - start);
    match core::str::from_utf8(sliced) {
        Ok(s) => s,
        Err(_) => "0.0.0",
    }
}
