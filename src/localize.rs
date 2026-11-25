//! Source localization-file parsing.
//!
//! The original launcher fails to load `*_english.txt` because the files are
//! UTF-16LE and its parser reads them as UTF-8, yielding zero entries. This
//! module detects the encoding from the BOM and decodes correctly, which is the
//! fix the report describes.

/// Detected text encoding of a localization file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    /// Detects the encoding from a leading byte-order mark.
    pub fn detect(bytes: &[u8]) -> Self {
        match bytes {
            [0xEF, 0xBB, 0xBF, ..] => Encoding::Utf8Bom,
            [0xFF, 0xFE, ..] => Encoding::Utf16Le,
            [0xFE, 0xFF, ..] => Encoding::Utf16Be,
            _ => Encoding::Utf8,
        }
    }
}

/// A single `"Key" "Value"` pair from a localization table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: String,
}

/// Decodes raw bytes to a `String`, honouring the BOM.
///
/// Returns `None` when the payload is not valid for the detected encoding.
pub fn decode(bytes: &[u8]) -> Option<String> {
    match Encoding::detect(bytes) {
        Encoding::Utf8Bom => std::str::from_utf8(&bytes[3..]).ok().map(str::to_owned),
        Encoding::Utf8 => std::str::from_utf8(bytes).ok().map(str::to_owned),
        Encoding::Utf16Le => decode_utf16(&bytes[2..], u16::from_le_bytes),
        Encoding::Utf16Be => decode_utf16(&bytes[2..], u16::from_be_bytes),
    }
}

fn decode_utf16(bytes: &[u8], to_u16: fn([u8; 2]) -> u16) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| to_u16(*c))
        .collect();
    String::from_utf16(&units).ok()
}

/// Parses `"Key" "Value"` pairs out of a localization table body.
///
/// Comment lines (`//`) and the `"lang" { ... }` wrapper are handled the same
/// way Source does: any line with two quoted strings contributes an entry.
pub fn parse_entries(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some((key, value)) = first_two_quoted(trimmed) {
            if key.is_empty() || key.eq_ignore_ascii_case("Language") || key == "Tokens" {
                continue;
            }
            out.push(Entry {
                key: key.to_owned(),
                value: value.to_owned(),
            });
        }
    }
    out
}

/// Convenience: decode then parse, reporting the detected encoding.
pub fn parse_file(bytes: &[u8]) -> Option<(Encoding, Vec<Entry>)> {
    let encoding = Encoding::detect(bytes);
    let text = decode(bytes)?;
    Some((encoding, parse_entries(&text)))
}

fn first_two_quoted(line: &str) -> Option<(&str, &str)> {
    let mut iter = QuotedIter { rest: line };
    let key = iter.next()?;
    let value = iter.next()?;
    Some((key, value))
}

struct QuotedIter<'a> {
    rest: &'a str,
}

impl<'a> Iterator for QuotedIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let start = self.rest.find('"')? + 1;
        let tail = &self.rest[start..];
        let end = tail.find('"')?;
        self.rest = &tail[end + 1..];
        Some(&tail[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTF16LE_SAMPLE: &[u8] = &[
        0xFF, 0xFE, // BOM
        b'"', 0x00, b'l', 0x00, b'a', 0x00, b'n', 0x00, b'g', 0x00, b'"', 0x00, b' ', 0x00, b'{',
        0x00, 0x0D, 0x00, 0x0A, 0x00, b'"', 0x00, b'K', 0x00, b'e', 0x00, b'y', 0x00, b'"', 0x00,
        b' ', 0x00, b'"', 0x00, b'V', 0x00, b'a', 0x00, b'l', 0x00, b'"', 0x00, 0x0D, 0x00, 0x0A,
        0x00, b'}', 0x00,
    ];

    #[test]
    fn detects_boms() {
        assert_eq!(
            Encoding::detect(&[0xEF, 0xBB, 0xBF, b'a']),
            Encoding::Utf8Bom
        );
        assert_eq!(
            Encoding::detect(&[0xFF, 0xFE, 0x41, 0x00]),
            Encoding::Utf16Le
        );
        assert_eq!(
            Encoding::detect(&[0xFE, 0xFF, 0x00, 0x41]),
            Encoding::Utf16Be
        );
        assert_eq!(Encoding::detect(b"plain"), Encoding::Utf8);
    }

    #[test]
    #[allow(invalid_from_utf8)] // intentional: the point is that these bytes are not UTF-8
    fn utf16le_decodes_where_utf8_fails() {
        // Reading the raw bytes as UTF-8 cannot yield the expected text: every
        // ASCII character is followed by a NUL byte.
        assert!(std::str::from_utf8(UTF16LE_SAMPLE).is_err());

        let (encoding, entries) = parse_file(UTF16LE_SAMPLE).expect("decodes");
        assert_eq!(encoding, Encoding::Utf16Le);
        assert_eq!(
            entries,
            vec![Entry {
                key: "Key".into(),
                value: "Val".into()
            }]
        );
    }

    #[test]
    fn parses_plain_utf8_table() {
        let text =
            "\"lang\"\n{\n\"Tokens\"\n{\n\"A\" \"Alpha\"\n// comment\n\"B\"\t\"Beta\"\n}\n}\n";
        let entries = parse_entries(text);
        assert_eq!(
            entries,
            vec![
                Entry {
                    key: "A".into(),
                    value: "Alpha".into()
                },
                Entry {
                    key: "B".into(),
                    value: "Beta".into()
                },
            ]
        );
    }

    #[test]
    fn odd_length_utf16_is_rejected() {
        assert!(decode(&[0xFF, 0xFE, 0x41]).is_none());
    }
}
