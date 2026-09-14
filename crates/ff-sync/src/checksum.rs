use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChecksumError {
    #[error("reading {path} to checksum it failed: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Hex-encoded SHA-256 of `bytes`, for verifying a downloaded bundle
/// against the checksum published in a [`crate::CycleManifest`] before
/// swapping it in as the active cycle (DESIGN.md §8).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

/// Same digest as [`sha256_hex`], streamed from a file rather than from a
/// slice. A real cycle bundle is ~145MB (DESIGN.md §11); on Android — the
/// one client that actually verifies one — reading that into a `Vec<u8>`
/// just to hash it is a 145MB allocation on a phone, on top of the copy
/// the download already wrote to disk. This reads it back in 64KiB chunks
/// instead, so peak memory is the buffer, not the bundle.
pub fn sha256_file_hex(path: &Path) -> Result<String, ChecksumError> {
    let io_err = |source: std::io::Error| ChecksumError::Io {
        path: path.display().to_string(),
        source,
    };
    let mut file = File::open(path).map_err(io_err)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).map_err(io_err)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

pub fn verify(bytes: &[u8], expected_sha256_hex: &str) -> bool {
    sha256_hex(bytes).eq_ignore_ascii_case(expected_sha256_hex)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn matches_a_known_sha256_vector() {
        // sha256("") == e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn verify_is_case_insensitive() {
        let digest = sha256_hex(b"freeflight");
        assert!(verify(b"freeflight", &digest.to_uppercase()));
        assert!(!verify(b"tampered", &digest));
    }

    #[test]
    fn streaming_file_digest_matches_the_in_memory_one() {
        // Deliberately larger than the 64KiB read buffer, so this covers
        // the multi-chunk path rather than a single short read.
        let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.sqlite");
        File::create(&path).unwrap().write_all(&payload).unwrap();

        assert_eq!(sha256_file_hex(&path).unwrap(), sha256_hex(&payload));
    }

    #[test]
    fn streaming_digest_of_a_missing_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        assert!(sha256_file_hex(&dir.path().join("nope.sqlite")).is_err());
    }
}
