use sha2::{Digest, Sha256};

pub(crate) fn sha256(value: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(value);
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for &byte in digest.iter() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::sha256;

    #[test]
    fn formats_digest_as_lowercase_sha256_identifier() {
        assert_eq!(
            sha256(b"postgresem"),
            "sha256:34a98f3650900159b2681c56695930eca9c34d6fdc40bc9bc98535af05bc089c"
        );
    }
}
