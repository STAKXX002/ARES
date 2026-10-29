//! Git integration. Read-only by design.

/// True for a full-length lowercase hexadecimal object id (SHA-1 or SHA-256).
pub fn is_object_id(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub mod runner;
pub mod status;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod object_id_tests {
    use super::is_object_id;

    #[test]
    fn only_full_length_lowercase_hex_is_an_object_id() {
        assert!(is_object_id(&"a".repeat(40)));
        assert!(is_object_id(&"0".repeat(64)));
        for bad in [
            "",
            "abc",
            &"a".repeat(39),
            &"a".repeat(41),
            &"A".repeat(40),
            &"g".repeat(40),
            &format!("{} ", "a".repeat(39)),
            &format!("{}\n", "a".repeat(39)),
            &"\u{e9}".repeat(20),
        ] {
            assert!(!is_object_id(bad), "{bad:?}");
        }
    }
}
