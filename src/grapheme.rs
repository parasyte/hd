use unicode_segmentation::UnicodeSegmentation;

/// A grapheme cluster.
///
/// One single-wide or double-wide character, potentially composed of multiple Unicode codepoints.
pub(crate) struct Span<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) parsed: Option<&'a str>,
}

impl Span<'_> {
    /// Create an ASCII span.
    pub(crate) fn ascii(bytes: &[u8]) -> Span<'_> {
        Span {
            bytes,
            parsed: None,
        }
    }

    /// Parse the first available grapheme cluster from a byte slice if possible.
    pub(crate) fn parse(bytes: &[u8]) -> Option<Span<'_>> {
        let s = std::str::from_utf8(bytes).ok()?;
        let mut graphemes = UnicodeSegmentation::graphemes(s, true);

        graphemes.next().map(|parsed| Span {
            bytes: &bytes[..parsed.len()],
            parsed: Some(parsed),
        })
    }

    /// Show a parsed grapheme cluster in the character table.
    pub(crate) fn as_char(&self, index: usize, column: usize, width: usize) -> Char<'_> {
        // Correctly handle row wrapping with double-wide characters.
        let cluster = self.parsed.unwrap();
        let wide = unicode_display_width::width(cluster) == 2;
        if (index == 0 && (!wide || column != width - 1)) || (index == 1 && wide && column == 0) {
            Char::Cluster(cluster)
        } else if wide && ((index == 1 && column != 0) || (index == 2 && column == 1)) {
            Char::Skip
        } else {
            Char::Space
        }
    }
}

/// How to show a span in the character table.
pub(crate) enum Char<'a> {
    /// Show the grapheme cluster.
    Cluster(&'a str),

    /// Show a blank space.
    Space,

    /// Skip this column.
    Skip,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_ascii() {
        let astronaut = "👩🏻‍🚀".as_bytes();
        let span = Span::parse(astronaut).unwrap();

        // Normal cases: grapheme cluster is shown on first line.
        for j in 0..7 {
            assert!(matches!(span.as_char(0, j, 8), Char::Cluster(_)));
            assert!(matches!(span.as_char(1, (j + 1) % 8, 8), Char::Skip));
            for i in 2..astronaut.len() {
                assert!(matches!(span.as_char(i, (j + i) % 8, 8), Char::Space));
            }
        }

        // Edge case: grapheme cluster is shown on second line.
        assert!(matches!(span.as_char(0, 7, 8), Char::Space));
        assert!(matches!(span.as_char(1, 0, 8), Char::Cluster(_)));
        assert!(matches!(span.as_char(2, 1, 8), Char::Skip));
        for i in 3..astronaut.len() {
            assert!(matches!(span.as_char(i, (i - 2) % 8, 8), Char::Space));
        }
    }
}
