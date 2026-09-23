//! Unicode-grapheme-aware editing of uncommitted composition text.

use unicode_segmentation::UnicodeSegmentation;

/// Editable composition text with a cursor measured in grapheme clusters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Composer {
    text: String,
    cursor_grapheme: usize,
}

/// Minimal state required to restore an immediately failed commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerSnapshot {
    text: String,
    cursor_grapheme: usize,
}

impl Composer {
    /// Creates an empty Composer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts an arbitrary UTF-8 string at the grapheme cursor.
    pub fn insert_text(&mut self, inserted: &str) {
        if inserted.is_empty() {
            return;
        }

        let insertion_byte = self.cursor_utf8_byte_offset();
        self.text.insert_str(insertion_byte, inserted);
        let byte_after_inserted_text = insertion_byte + inserted.len();
        self.cursor_grapheme = grapheme_cursor_after_byte(&self.text, byte_after_inserted_text);
    }

    /// Deletes one complete grapheme before the cursor.
    pub fn backspace(&mut self) -> bool {
        if self.cursor_grapheme == 0 {
            return false;
        }

        let end = self.cursor_utf8_byte_offset();
        let start = byte_offset_for_grapheme(&self.text, self.cursor_grapheme - 1);
        self.text.replace_range(start..end, "");
        self.cursor_grapheme = grapheme_cursor_after_byte(&self.text, start);
        true
    }

    /// Deletes one complete grapheme after the cursor.
    pub fn delete_forward(&mut self) -> bool {
        let grapheme_count = self.grapheme_count();
        if self.cursor_grapheme >= grapheme_count {
            return false;
        }

        let start = self.cursor_utf8_byte_offset();
        let end = byte_offset_for_grapheme(&self.text, self.cursor_grapheme + 1);
        self.text.replace_range(start..end, "");
        self.cursor_grapheme = grapheme_cursor_after_byte(&self.text, start);
        true
    }

    /// Moves the cursor by grapheme clusters and clamps it to the composition.
    pub fn move_cursor(&mut self, grapheme_delta: i32) -> bool {
        let old = self.cursor_grapheme;
        let max = self.grapheme_count() as i64;
        let next = (old as i64 + i64::from(grapheme_delta)).clamp(0, max);
        self.cursor_grapheme = next as usize;
        self.cursor_grapheme != old
    }

    /// Clears text and returns the cursor to zero.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor_grapheme = 0;
    }

    /// Returns the current UTF-8 composition.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the cursor in extended grapheme clusters.
    pub const fn cursor_grapheme(&self) -> usize {
        self.cursor_grapheme
    }

    /// Returns the cursor as a UTF-8 byte offset into `text()`.
    pub fn cursor_utf8_byte_offset(&self) -> usize {
        byte_offset_for_grapheme(&self.text, self.cursor_grapheme)
    }

    /// Returns whether the composition is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Returns the number of extended grapheme clusters.
    pub fn grapheme_count(&self) -> usize {
        self.text.graphemes(true).count()
    }

    pub(crate) fn recovery_snapshot(&self) -> ComposerSnapshot {
        ComposerSnapshot {
            text: self.text.clone(),
            cursor_grapheme: self.cursor_grapheme,
        }
    }

    pub(crate) fn restore(&mut self, snapshot: ComposerSnapshot) {
        self.text = snapshot.text;
        self.cursor_grapheme = snapshot.cursor_grapheme;
    }
}

fn byte_offset_for_grapheme(text: &str, grapheme_index: usize) -> usize {
    text.grapheme_indices(true)
        .nth(grapheme_index)
        .map_or(text.len(), |(byte, _)| byte)
}

fn grapheme_cursor_after_byte(text: &str, target_byte: usize) -> usize {
    if target_byte == 0 {
        return 0;
    }

    for (index, (start, grapheme)) in text.grapheme_indices(true).enumerate() {
        if target_byte <= start {
            return index;
        }
        if target_byte <= start + grapheme.len() {
            return index + 1;
        }
    }
    text.graphemes(true).count()
}

#[cfg(test)]
mod tests {
    use super::Composer;

    #[test]
    fn inserts_ascii_and_utf8_text() {
        let mut composer = Composer::new();
        composer.insert_text("hello");
        composer.insert_text("你好");

        assert_eq!(composer.text(), "hello你好");
        assert_eq!(composer.cursor_grapheme(), 7);
        assert_eq!(composer.cursor_utf8_byte_offset(), composer.text().len());
    }

    #[test]
    fn inserts_emoji_as_one_grapheme() {
        let mut composer = Composer::new();
        composer.insert_text("😀");

        assert_eq!(composer.grapheme_count(), 1);
        assert_eq!(composer.cursor_grapheme(), 1);
    }

    #[test]
    fn backspace_deletes_entire_zwj_emoji() {
        let mut composer = Composer::new();
        composer.insert_text("A👨‍👩‍👧‍👦B");

        assert!(composer.backspace());
        assert_eq!(composer.text(), "A👨‍👩‍👧‍👦");
        assert!(composer.backspace());
        assert_eq!(composer.text(), "A");
    }

    #[test]
    fn backspace_deletes_combining_sequence_once() {
        let mut composer = Composer::new();
        composer.insert_text("a\u{301}");

        assert_eq!(composer.grapheme_count(), 1);
        assert!(composer.backspace());
        assert!(composer.is_empty());
    }

    #[test]
    fn inserting_combining_mark_keeps_valid_cursor() {
        let mut composer = Composer::new();
        composer.insert_text("a");
        composer.insert_text("\u{301}");

        assert_eq!(composer.text(), "a\u{301}");
        assert_eq!(composer.grapheme_count(), 1);
        assert_eq!(composer.cursor_grapheme(), 1);
    }

    #[test]
    fn moves_cursor_and_inserts_in_middle() {
        let mut composer = Composer::new();
        composer.insert_text("nihao");
        assert!(composer.move_cursor(-1));
        composer.insert_text("X");

        assert_eq!(composer.text(), "nihaXo");
        assert_eq!(composer.cursor_grapheme(), 5);
    }

    #[test]
    fn delete_forward_removes_full_grapheme() {
        let mut composer = Composer::new();
        composer.insert_text("A👨‍👩‍👧‍👦B");
        composer.move_cursor(-2);

        assert!(composer.delete_forward());
        assert_eq!(composer.text(), "AB");
        assert_eq!(composer.cursor_grapheme(), 1);
    }

    #[test]
    fn empty_backspace_is_a_no_op() {
        let mut composer = Composer::new();
        assert!(!composer.backspace());
        assert_eq!(composer.cursor_grapheme(), 0);
    }

    #[test]
    fn cursor_movement_clamps_to_bounds() {
        let mut composer = Composer::new();
        composer.insert_text("abc");

        assert!(composer.move_cursor(-99));
        assert_eq!(composer.cursor_grapheme(), 0);
        assert!(!composer.move_cursor(-1));
        assert!(composer.move_cursor(99));
        assert_eq!(composer.cursor_grapheme(), 3);
        assert!(!composer.move_cursor(1));
    }

    #[test]
    fn clear_resets_text_and_cursor() {
        let mut composer = Composer::new();
        composer.insert_text("你好");
        composer.clear();

        assert!(composer.is_empty());
        assert_eq!(composer.cursor_grapheme(), 0);
    }
}
