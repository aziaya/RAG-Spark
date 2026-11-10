use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha256_is_hex_of_32_bytes() {
        let h = super::sha256_hex(b"abc");
        assert_eq!(h.len(), 64);
        assert!(h.starts_with("ba7816bf"));
    }
}
