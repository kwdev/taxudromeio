//! IMAP modified UTF-7 conversion at the wire boundary (RFC 3501 section 5.1.3).
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};

pub(crate) fn encode(name: &str) -> String {
    let mut result = String::new();
    let mut shifted = Vec::new();
    fn flush(shifted: &mut Vec<u8>, result: &mut String) {
        if !shifted.is_empty() {
            result.push('&');
            result.push_str(&STANDARD_NO_PAD.encode(&*shifted).replace('/', ","));
            result.push('-');
            shifted.clear();
        }
    }
    for ch in name.chars() {
        if (' '..='~').contains(&ch) {
            flush(&mut shifted, &mut result);
            if ch == '&' {
                result.push_str("&-");
            } else {
                result.push(ch);
            }
        } else {
            let mut units = [0; 2];
            for unit in ch.encode_utf16(&mut units) {
                shifted.extend_from_slice(&unit.to_be_bytes());
            }
        }
    }
    flush(&mut shifted, &mut result);
    result
}

pub(crate) fn decode(name: &str) -> Result<String> {
    ensure!(
        name.is_ascii(),
        "Server returned a mailbox outside the negotiated IMAP encoding"
    );
    let mut result = String::new();
    let mut rest = name;
    while let Some(start) = rest.find('&') {
        result.push_str(&rest[..start]);
        let shifted = &rest[start + 1..];
        let end = shifted
            .find('-')
            .context("Invalid modified UTF-7 mailbox name")?;
        if end == 0 {
            result.push('&');
        } else {
            let bytes = STANDARD_NO_PAD.decode(shifted[..end].replace(',', "/"))?;
            ensure!(bytes.len() % 2 == 0, "Invalid UTF-16 mailbox name");
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            result.push_str(&String::from_utf16(&units)?);
        }
        rest = &shifted[end + 1..];
    }
    result.push_str(rest);
    ensure!(
        !result.chars().any(char::is_control),
        "Control characters in mailbox name"
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_example_and_unicode_roundtrip() {
        let decoded = "~peter/mail/台北/日本語";
        let encoded = "~peter/mail/&U,BTFw-/&ZeVnLIqe-";
        assert_eq!(encode(decoded), encoded);
        assert_eq!(decode(encoded).unwrap(), decoded);
        for name in ["Projects", "R&D/Drafts", "Αλληλογραφία/🦀", "Entwürfe", "&"] {
            assert_eq!(decode(&encode(name)).unwrap(), name);
        }
        for invalid in ["&broken", "&a-", "&2AA-", "&AAo-"] {
            assert!(decode(invalid).is_err());
        }
    }
}
