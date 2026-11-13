//! Small hashing helpers shared by the asset store and the markdown pipeline.
//!
//! The content-addressed image names and the Mermaid figure digests use the
//! same MD5 hex encoding as the original generator, so they stay stable.

use std::fmt::Write as _;

use md5::Digest;

/// Lowercase hex MD5 digest of `bytes`.
pub(crate) fn md5_hex(bytes: &[u8]) -> String {
    let digest = md5::Md5::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Writing to a `String` cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digests() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"hello"), "5d41402abc4b2a76b9719d911017c592");
    }
}
