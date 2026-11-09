//! Byte offsets (the syntax provider's coordinates) and editor positions
//! (0-based lines, UTF-16 columns: the other providers') of one text.

use openidea_plugin::{Position, Range};

pub struct LineIndex {
    /// Byte offset of each line's start; lines end at `\n`.
    starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = Vec::with_capacity(text.len() / 32 + 1);
        starts.push(0);
        let mut at = 0;
        while let Some(k) = text[at..].find('\n') {
            at += k + 1;
            starts.push(at as u32);
        }
        LineIndex { starts }
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// The 0-based line holding the byte offset.
    pub fn line_of(&self, offset: usize) -> u32 {
        (self.starts.partition_point(|&s| s as usize <= offset) - 1) as u32
    }

    pub fn line_start(&self, line: u32) -> u32 {
        self.starts.get(line as usize).copied().unwrap_or_else(|| *self.starts.last().unwrap_or(&0))
    }

    /// The end of a line's text, before its `\n` (and a `\r` before it).
    pub fn line_end(&self, text: &str, line: u32) -> usize {
        let end = self.starts.get(line as usize + 1).map_or(text.len(), |&s| s as usize - 1);
        if end > 0 && text.as_bytes().get(end - 1) == Some(&b'\r') && end < text.len() { end - 1 } else { end }
    }

    pub fn position(&self, text: &str, offset: usize) -> Position {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.line_of(offset);
        let start = self.line_start(line) as usize;
        let segment = &text[start..offset];
        let character =
            if segment.is_ascii() { segment.len() as u32 } else { segment.chars().map(|c| c.len_utf16() as u32).sum() };
        Position::new(line, character)
    }

    /// The byte offset of a position; a column past the line's end is clamped to it.
    pub fn offset(&self, text: &str, pos: Position) -> usize {
        if pos.line as usize >= self.starts.len() {
            return text.len();
        }
        let start = self.line_start(pos.line) as usize;
        let end = self.line_end(text, pos.line);
        let mut units = 0u32;
        for (i, c) in text[start..end].char_indices() {
            if units >= pos.character {
                return start + i;
            }
            units += c.len_utf16() as u32;
        }
        end
    }

    pub fn range(&self, text: &str, start: usize, end: usize) -> Range {
        Range::new(self.position(text, start), self.position(text, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_columns_round_trip() {
        let text = "č: 😀x\r\nb: 1\n";
        let lines = LineIndex::new(text);
        let x = text.find('x').unwrap();
        assert_eq!(lines.position(text, x), Position::new(0, 5), "č is one unit, 😀 two");
        assert_eq!(lines.offset(text, Position::new(0, 5)), x);
        assert_eq!(lines.offset(text, Position::new(0, 99)), x + 1, "clamped before \\r\\n");
        assert_eq!(lines.position(text, text.find('b').unwrap()), Position::new(1, 0));
        assert_eq!(lines.offset(text, Position::new(9, 0)), text.len());
        assert_eq!(lines.position(text, 1), Position::new(0, 0), "inside č: its start");
        assert_eq!(lines.line_count(), 3);
    }
}
