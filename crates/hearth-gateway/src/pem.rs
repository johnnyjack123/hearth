//! Certificates in PEM files and base64, without dependencies.
//!
//! The host reads the root certificate of a server with its own CA from a
//! PEM file and sends it to the gateway as base64 DER over the control
//! protocol. Both ends validate it with these functions; the gateway's TLS
//! stack parses it fully when it builds its trust store.

/// Most certificates accepted as trust anchors for one server.
pub const MAX_CERTIFICATES: usize = 8;

/// Upper bound for the DER size of all certificates together. A root
/// certificate is 0.5–2 KiB; the limit keeps the `llm` control line, which
/// carries them base64-encoded, well below [`crate::control::MAX_LINE`].
pub const MAX_CERTIFICATE_BYTES: usize = 24 * 1024;

const BEGIN: &str = "-----BEGIN ";
const END: &str = "-----END ";

/// The DER bodies of all `CERTIFICATE` blocks in a PEM text, in order.
/// Other blocks (keys, parameters) and text around the blocks are ignored,
/// like `openssl` does; a file without any certificate is an error.
pub fn certificates(text: &str) -> Result<Vec<Vec<u8>>, String> {
    let certificates = blocks(text, "CERTIFICATE")?;
    if certificates.is_empty() {
        return Err("keine Zertifikate (-----BEGIN CERTIFICATE-----) gefunden".to_owned());
    }
    check_certificates(&certificates)?;
    Ok(certificates)
}

/// The decoded bodies of all blocks labelled `label` (`CERTIFICATE`,
/// `PRIVATE KEY`, …) in a PEM text.
pub fn blocks(text: &str, label: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut found = Vec::new();
    let mut lines = text.lines().map(str::trim);
    while let Some(line) = lines.next() {
        let Some(begin) = line
            .strip_prefix(BEGIN)
            .and_then(|rest| rest.strip_suffix("-----"))
        else {
            continue;
        };
        let mut body = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if let Some(end) = line
                .strip_prefix(END)
                .and_then(|rest| rest.strip_suffix("-----"))
            {
                if end != begin {
                    return Err(format!("PEM-Block {begin:?} endet mit {end:?}"));
                }
                closed = true;
                break;
            }
            // RFC 7468 allows headers only in legacy encrypted blocks, which
            // are never certificates.
            if line.contains(':') {
                return Err(format!("PEM-Block {begin:?} mit Kopfzeilen"));
            }
            body.push_str(line);
        }
        if !closed {
            return Err(format!("PEM-Block {begin:?} ist nicht abgeschlossen"));
        }
        if begin == label {
            let der = base64_decode(&body)
                .ok_or_else(|| format!("PEM-Block {begin:?} ist kein gültiges Base64"))?;
            found.push(der);
        }
    }
    Ok(found)
}

/// Rough check of certificates before they travel or are trusted: count,
/// total size, and that each is one complete DER `SEQUENCE`. The real
/// parsing happens in the TLS stack.
pub fn check_certificates(certificates: &[Vec<u8>]) -> Result<(), String> {
    if certificates.is_empty() || certificates.len() > MAX_CERTIFICATES {
        return Err(format!(
            "erwartet 1 bis {MAX_CERTIFICATES} Zertifikate, nicht {}",
            certificates.len()
        ));
    }
    let total: usize = certificates.iter().map(Vec::len).sum();
    if total > MAX_CERTIFICATE_BYTES {
        return Err(format!(
            "Zertifikate zu groß ({total} Bytes, höchstens {MAX_CERTIFICATE_BYTES})"
        ));
    }
    if let Some(index) = certificates.iter().position(|der| !is_der_sequence(der)) {
        return Err(format!("Zertifikat {} ist kein DER-Zertifikat", index + 1));
    }
    Ok(())
}

/// One DER `SEQUENCE` whose length field covers exactly the rest.
fn is_der_sequence(der: &[u8]) -> bool {
    let [0x30, first, rest @ ..] = der else {
        return false;
    };
    let (length, body) = if first & 0x80 == 0 {
        (usize::from(*first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 3 || rest.len() < count {
            return false;
        }
        let (bytes, body) = rest.split_at(count);
        let length = bytes
            .iter()
            .fold(0usize, |acc, byte| (acc << 8) | usize::from(*byte));
        (length, body)
    };
    length > 0 && length == body.len()
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (RFC 4648, section 4).
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let bits = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(
                    ALPHABET[(bits >> (18 - 6 * index)) as usize & 63],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard base64 with padding; `None` for anything else,
/// including non-canonical trailing bits. Whitespace is not allowed.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let padding = chunk.iter().rev().take_while(|b| **b == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return None;
        }
        let mut bits = 0u32;
        for byte in &chunk[..4 - padding] {
            let value = ALPHABET.iter().position(|a| a == byte)?;
            bits = (bits << 6) | value as u32;
        }
        bits <<= 6 * padding;
        let decoded = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        // Bits that padding drops must be zero, so every byte string has
        // exactly one encoding.
        if decoded[3 - padding..].iter().any(|b| *b != 0) {
            return None;
        }
        out.extend_from_slice(&decoded[..3 - padding]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny but well-formed DER sequence, enough for the rough check.
    fn der(len: usize) -> Vec<u8> {
        let mut der = vec![0x30];
        if len < 128 {
            der.push(len as u8);
        } else {
            der.extend([0x82, (len >> 8) as u8, len as u8]);
        }
        der.extend(std::iter::repeat_n(7, len));
        der
    }

    fn pem(label: &str, data: &[u8]) -> String {
        let encoded = base64_encode(data);
        let lines: Vec<&str> = encoded
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
            lines.join("\n")
        )
    }

    #[test]
    fn base64_round_trip() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), encoded);
            assert_eq!(base64_decode(encoded).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)).unwrap(), all);
        for bad in [
            "Zg=",
            "Zg===",
            "Z===",
            "Zm9v Yg==",
            "Zg==Zg==",
            "Zh==",
            "Zm9-",
            "Zm,v",
        ] {
            assert_eq!(base64_decode(bad), None, "{bad:?} accepted");
        }
    }

    #[test]
    fn certificates_from_pem() {
        let one = der(100);
        let two = der(300);
        let text = format!(
            "Subject: CN=Test\r\n{}junk between\n{}{}",
            pem("CERTIFICATE", &one),
            pem("PRIVATE KEY", &der(10)),
            pem("CERTIFICATE", &two).replace('\n', "\r\n")
        );
        assert_eq!(certificates(&text).unwrap(), [one.clone(), two]);
        assert_eq!(blocks(&text, "PRIVATE KEY").unwrap(), [der(10)]);

        for bad in [
            String::new(),
            pem("PRIVATE KEY", &der(10)),
            "-----BEGIN CERTIFICATE-----\nAAAA\n".to_owned(),
            "-----BEGIN CERTIFICATE-----\nAAAA\n-----END KEY-----\n".to_owned(),
            "-----BEGIN CERTIFICATE-----\nProc-Type: 4,ENCRYPTED\n-----END CERTIFICATE-----\n"
                .to_owned(),
            pem("CERTIFICATE", b"not der"),
            pem("CERTIFICATE", &one).replace("MG", "M!"),
            pem("CERTIFICATE", &[&one[..], &[0]].concat()),
            pem("CERTIFICATE", &der(MAX_CERTIFICATE_BYTES)),
            pem("CERTIFICATE", &one).repeat(MAX_CERTIFICATES + 1),
        ] {
            assert!(certificates(&bad).is_err(), "{bad:?} accepted");
        }
    }
}
