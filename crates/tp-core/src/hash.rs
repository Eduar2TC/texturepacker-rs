//! Pixel hashing for alias (duplicate) detection.
//!
//! Uses xxh3 (extremely fast, 64-bit) over the trimmed RGBA buffer. Hash
//! collisions are resolved by a byte-for-byte comparison of the buffers, so
//! aliasing is always exact.

use xxhash_rust::xxh3::xxh3_64;

/// Hash a slice of RGBA pixels into a hex string.
pub fn hash_pixels_rgba(pixels: &[u8]) -> String {
    format!("{:016x}", xxh3_64(pixels))
}

/// A global table mapping pixel-hash -> first-seen sprite id.
///
/// The pipeline keeps one of these across all sprites so duplicates are
/// resolved against the *first* sprite that produced the same content.
#[derive(Debug, Default)]
pub struct AliasTable {
    /// hash -> (first sprite id, checksum of buffer length, buffer)
    entries: std::collections::HashMap<String, (String, usize, Vec<u8>)>,
}

impl AliasTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up `(pixels, width, height)`; if the content was already seen,
    /// return the id of the first sprite (`Some`). Otherwise register this
    /// sprite as the canonical owner and return `None`.
    ///
    /// The comparison is byte-exact, so two sprites are aliases only when
    /// their trimmed buffers are identical (same size *and* same pixels).
    pub fn lookup_or_register(
        &mut self,
        hash: &str,
        id: &str,
        pixels: &[u8],
        width: usize,
        height: usize,
    ) -> Option<String> {
        let expected_len = width * height * 4;
        debug_assert_eq!(pixels.len(), expected_len);

        if let Some((owner, len, data)) = self.entries.get(hash) {
            if *len == expected_len && data.as_slice() == pixels {
                return Some(owner.clone());
            }
        }
        self.entries.insert(
            hash.to_string(),
            (id.to_string(), expected_len, pixels.to_vec()),
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_buffers_are_aliases() {
        let mut table = AliasTable::new();
        let a = vec![10u8; 4 * 4 * 4];
        let h = hash_pixels_rgba(&a);
        assert_eq!(table.lookup_or_register(&h, "a", &a, 4, 4), None);
        assert_eq!(
            table.lookup_or_register(&h, "b", &a, 4, 4),
            Some("a".into())
        );
    }

    #[test]
    fn different_size_same_hash_is_not_alias() {
        let mut table = AliasTable::new();
        let a = vec![7u8; 4 * 4 * 4];
        let h = hash_pixels_rgba(&a);
        assert_eq!(table.lookup_or_register(&h, "a", &a, 4, 4), None);
        let b = vec![7u8; 4 * 4 * 3];
        assert_eq!(table.lookup_or_register(&h, "b", &b, 4, 3), None);
    }
}
