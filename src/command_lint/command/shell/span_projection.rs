//! Character-to-UTF-8-byte span projection and span hull arithmetic for shell lowering.

use brush_parser::SourceSpan as BrushSpan;

use crate::diagnostic::SourceSpan;

/// Maps `brush_parser`'s 0-based Unicode `char` offset to 0-based UTF-8 byte offsets, shifted by
/// `base_byte_offset` when parsing nested command substitutions.
pub(super) struct CharToByteMap {
    non_ascii_offsets: Option<Vec<usize>>,
    byte_len: usize,
    base_byte_offset: usize,
}

impl CharToByteMap {
    pub(super) fn new(source: &str, base_byte_offset: usize) -> Self {
        let non_ascii_offsets = (!source.is_ascii()).then(|| {
            let mut offsets: Vec<usize> = source
                .char_indices()
                .map(|(byte_offset, _)| byte_offset)
                .collect();
            offsets.push(source.len());
            offsets
        });
        Self {
            non_ascii_offsets,
            byte_len: source.len(),
            base_byte_offset,
        }
    }

    pub(super) fn map_span(&self, brush_span: &BrushSpan) -> SourceSpan {
        SourceSpan::new(
            self.char_to_byte(brush_span.start.index),
            self.char_to_byte(brush_span.end.index),
        )
    }

    fn char_to_byte(&self, char_offset: usize) -> usize {
        let local_offset = self.non_ascii_offsets.as_ref().map_or_else(
            || char_offset.min(self.byte_len),
            |offsets| offsets.get(char_offset).copied().unwrap_or(self.byte_len),
        );
        self.base_byte_offset + local_offset
    }
}

/// Smallest span enclosing both `left` and `right`.
pub(super) const fn hull_span(left: SourceSpan, right: SourceSpan) -> SourceSpan {
    let start = if left.start < right.start {
        left.start
    } else {
        right.start
    };
    let end = if left.end > right.end {
        left.end
    } else {
        right.end
    };
    SourceSpan::new(start, end)
}
