use crate::{grapheme::Span, Numeric};

/// Byte slices are grouped into spans by [`Kind`].
pub(crate) struct Group<'a> {
    /// The kind of group this is.
    pub(crate) kind: Kind,

    /// The span of the byte slice composing the entire group.
    pub(crate) span: Span<'a>,
}

/// Byte classifications for pretty printing.
#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum Kind {
    /// Numeric characters, depending on [`Numeric`] context:
    ///
    /// - Octal decimal: `0x30..=0x37`
    /// - Decimal: `0x30..=0x39`
    /// - Hexadecimal: `0x30..=0x39`, `0x41..=0x46`, and `0x61..=0x66`
    Numeric,

    /// ASCII printable characters: `0x20..=0x7e`
    Printable,

    /// ASCII control characters: `0x00..=0x1f` and `0x7f`
    Control,

    /// UTF-8 encoded grapheme cluster (e.g. emoji).
    Graphemes,

    /// Invalid ASCII/UTF-8 characters: `0x80..=0xff`
    Invalid,
}

impl Group<'_> {
    /// Parse a group (span and classification) from a byte slice.
    pub(crate) fn gather(bytes: &[u8], numeric: Numeric) -> Group<'_> {
        debug_assert!(!bytes.is_empty(), "Cannot gather an empty byte slice");
        let byte = bytes[0];

        if Kind::is_numeric(byte, numeric) {
            Self::numeric_span(bytes, numeric)
        } else if Kind::is_printable(byte) {
            Self::printable_span(bytes, numeric)
        } else if Kind::is_control(byte) {
            Self::control_span(bytes)
        } else if let Some(span) = Span::parse(bytes) {
            Group {
                kind: Kind::Graphemes,
                span,
            }
        } else {
            Self::invalid_span(bytes, numeric)
        }
    }

    fn new(kind: Kind, bytes: &[u8]) -> Group<'_> {
        Group {
            kind,
            span: Span::ascii(bytes),
        }
    }

    fn numeric_span(bytes: &[u8], numeric: Numeric) -> Group<'_> {
        let mut length = 1;
        for byte in &bytes[1..] {
            if !Kind::is_numeric(*byte, numeric) {
                break;
            }
            length += 1;
        }

        Self::new(Kind::Numeric, &bytes[..length])
    }

    fn printable_span(bytes: &[u8], numeric: Numeric) -> Group<'_> {
        let mut length = 1;
        for byte in &bytes[1..] {
            if !Kind::is_printable(*byte) || Kind::is_numeric(*byte, numeric) {
                break;
            }
            length += 1;
        }

        Self::new(Kind::Printable, &bytes[..length])
    }

    fn control_span(bytes: &[u8]) -> Group<'_> {
        let mut length = 1;
        for byte in &bytes[1..] {
            if !Kind::is_control(*byte) {
                break;
            }
            length += 1;
        }

        Self::new(Kind::Control, &bytes[..length])
    }

    fn invalid_span(bytes: &[u8], numeric: Numeric) -> Group<'_> {
        let mut length = 1;
        for (i, byte) in bytes[1..].iter().enumerate() {
            if Kind::is_numeric(*byte, numeric)
                || Kind::is_printable(*byte)
                || Kind::is_control(*byte)
                || Span::parse(&bytes[i..]).is_some()
            {
                break;
            }
            length += 1;
        }

        Self::new(Kind::Invalid, &bytes[..length])
    }
}

impl Kind {
    fn is_numeric(byte: u8, numeric: Numeric) -> bool {
        match numeric {
            Numeric::Octal => (b'0'..b'7').contains(&byte),
            Numeric::Decimal => byte.is_ascii_digit(),
            Numeric::Hexadecimal => byte.is_ascii_hexdigit(),
        }
    }

    fn is_printable(byte: u8) -> bool {
        byte == b' ' || byte.is_ascii_graphic()
    }

    fn is_control(byte: u8) -> bool {
        byte.is_ascii_control()
    }
}
