use sha2::{Digest, Sha256};

/// Hex-encoded SHA-256 of `bytes`, for verifying a downloaded bundle
/// against the checksum published in a [`crate::CycleManifest`] before
/// swapping it in as the active cycle (DESIGN.md §8).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
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
}
