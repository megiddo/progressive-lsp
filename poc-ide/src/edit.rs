//! `EditCommand`: insert, delete, select, cut, copy, paste.

use crate::buffer::{OpenBuffer, Selection};
use crate::error::IdeError;
use crate::ports::ClipboardPort;

/// Mutates an [`OpenBuffer`] rope only through this Command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditCommand {
    Insert { text: String },
    Delete,
    Select { selection: Selection },
    Cut,
    Copy,
    Paste,
}

impl EditCommand {
    pub fn insert(text: impl Into<String>) -> Self {
        Self::Insert { text: text.into() }
    }

    pub fn delete() -> Self {
        Self::Delete
    }

    pub fn select(selection: Selection) -> Self {
        Self::Select { selection }
    }

    pub fn cut() -> Self {
        Self::Cut
    }

    pub fn copy() -> Self {
        Self::Copy
    }

    pub fn paste() -> Self {
        Self::Paste
    }

    pub fn apply(
        &self,
        buffer: &mut OpenBuffer,
        clipboard: &mut impl ClipboardPort,
    ) -> Result<(), IdeError> {
        match self {
            Self::Insert { text } => {
                buffer.insert_text(text);
                Ok(())
            }
            Self::Delete => {
                buffer.delete_range_or_forward();
                Ok(())
            }
            Self::Select { selection } => {
                buffer.set_selection(*selection);
                Ok(())
            }
            Self::Cut => {
                clipboard.set_text(&buffer.selected_text())?;
                buffer.delete_selection_only();
                Ok(())
            }
            Self::Copy => {
                clipboard.set_text(&buffer.selected_text())?;
                Ok(())
            }
            Self::Paste => {
                let text = clipboard.get_text()?;
                buffer.insert_text(&text);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{FakeClipboard, MemFs};

    fn load(path: &str, bytes: &str) -> (OpenBuffer, MemFs) {
        let mut fs = MemFs::new();
        fs.add_file(path, bytes.as_bytes()).unwrap();
        let buf = OpenBuffer::load(path, &fs).unwrap();
        (buf, fs)
    }

    #[test]
    fn edit_command_insert_delete_mutates_rope_only() {
        let (mut buf, _) = load("/ws/a.rs", "abcd");
        let mut clip = FakeClipboard::new();
        EditCommand::select(Selection::collapsed(2))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::insert("XY")
            .apply(&mut buf, &mut clip)
            .unwrap();
        assert_eq!(buf.text(), "abXYcd");
        assert_eq!(buf.selection(), Selection::collapsed(4));
        assert!(buf.is_dirty());

        EditCommand::select(Selection::new(2, 4))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::insert("Z").apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "abZcd");
        assert_eq!(buf.selection(), Selection::collapsed(3));

        EditCommand::insert("").apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "abZcd");

        EditCommand::select(Selection::new(1, 3))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::delete().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "acd");
        assert_eq!(buf.selection(), Selection::collapsed(1));

        EditCommand::delete().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "ad");
        EditCommand::select(Selection::collapsed(buf.len_chars()))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::delete().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "ad");
        assert!(buf.is_dirty());
    }

    #[test]
    fn edit_command_empty_insert_or_delete_at_end_does_not_dirty() {
        let (mut buf, _) = load("/ws/a.rs", "ab");
        let mut clip = FakeClipboard::new();
        EditCommand::insert("").apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "ab");
        assert!(!buf.is_dirty());
        EditCommand::select(Selection::collapsed(2))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::delete().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "ab");
        assert!(!buf.is_dirty());
    }

    #[test]
    fn edit_command_select_does_not_set_dirty() {
        let (mut buf, _) = load("/ws/a.rs", "hi");
        let mut clip = FakeClipboard::new();
        assert!(!buf.is_dirty());
        EditCommand::select(Selection::new(0, 2))
            .apply(&mut buf, &mut clip)
            .unwrap();
        assert_eq!(buf.selection(), Selection::new(0, 2));
        assert!(!buf.is_dirty());
        EditCommand::select(Selection::new(9, 1))
            .apply(&mut buf, &mut clip)
            .unwrap();
        assert_eq!(buf.selection(), Selection::new(1, 2));
        assert!(!buf.is_dirty());
    }

    #[test]
    fn edit_command_cut_copy_paste_uses_clipboard_port() {
        let (mut buf, _) = load("/ws/a.rs", "hello world");
        let mut clip = FakeClipboard::new();
        EditCommand::select(Selection::new(0, 5))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::copy().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(clip.contents(), "hello");
        assert_eq!(buf.text(), "hello world");
        assert!(!buf.is_dirty());

        EditCommand::select(Selection::new(6, 11))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::cut().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(clip.contents(), "world");
        assert_eq!(buf.text(), "hello ");
        assert!(buf.is_dirty());
        assert_eq!(buf.selection(), Selection::collapsed(6));

        EditCommand::select(Selection::collapsed(0))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::paste().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "worldhello ");
        assert_eq!(buf.selection(), Selection::collapsed(5));

        EditCommand::select(Selection::new(0, 5))
            .apply(&mut buf, &mut clip)
            .unwrap();
        clip.set_text("X").unwrap();
        EditCommand::paste().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "Xhello ");
    }

    #[test]
    fn edit_command_cut_empty_selection_does_not_dirty() {
        let (mut buf, _) = load("/ws/a.rs", "ab");
        let mut clip = FakeClipboard::new();
        clip.set_text("old").unwrap();
        EditCommand::select(Selection::collapsed(1))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::cut().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(clip.contents(), "");
        assert_eq!(buf.text(), "ab");
        assert!(!buf.is_dirty());
    }

    #[test]
    fn edit_command_clipboard_error_does_not_mutate_rope() {
        let (mut buf, _) = load("/ws/a.rs", "abcd");
        let mut clip = FakeClipboard::new();
        EditCommand::select(Selection::new(1, 3))
            .apply(&mut buf, &mut clip)
            .unwrap();
        clip.fail_next_set();
        assert!(EditCommand::cut()
            .apply(&mut buf, &mut clip)
            .unwrap_err()
            .is_clipboard());
        assert_eq!(buf.text(), "abcd");
        assert!(!buf.is_dirty());
        assert_eq!(buf.selected_text(), "bc");

        clip.fail_next_set();
        assert!(EditCommand::copy()
            .apply(&mut buf, &mut clip)
            .unwrap_err()
            .is_clipboard());
        assert_eq!(buf.text(), "abcd");

        clip.fail_next_get();
        assert!(EditCommand::paste()
            .apply(&mut buf, &mut clip)
            .unwrap_err()
            .is_clipboard());
        assert_eq!(buf.text(), "abcd");
        assert!(!buf.is_dirty());
    }

    #[test]
    fn edit_command_unicode_uses_char_offsets() {
        let (mut buf, _) = load("/ws/a.rs", "café");
        let mut clip = FakeClipboard::new();
        assert_eq!(buf.len_chars(), 4);
        EditCommand::select(Selection::new(3, 4))
            .apply(&mut buf, &mut clip)
            .unwrap();
        assert_eq!(buf.selected_text(), "é");
        EditCommand::insert("e").apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "cafe");
        EditCommand::select(Selection::new(0, 2))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::cut().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(clip.contents(), "ca");
        EditCommand::select(Selection::collapsed(buf.len_chars()))
            .apply(&mut buf, &mut clip)
            .unwrap();
        EditCommand::paste().apply(&mut buf, &mut clip).unwrap();
        assert_eq!(buf.text(), "feca");
    }

    #[test]
    fn edit_command_constructors_match_variants() {
        assert_eq!(
            EditCommand::insert("x"),
            EditCommand::Insert { text: "x".into() }
        );
        assert_eq!(EditCommand::delete(), EditCommand::Delete);
        assert_eq!(
            EditCommand::select(Selection::collapsed(1)),
            EditCommand::Select {
                selection: Selection::collapsed(1)
            }
        );
        assert_eq!(EditCommand::cut(), EditCommand::Cut);
        assert_eq!(EditCommand::copy(), EditCommand::Copy);
        assert_eq!(EditCommand::paste(), EditCommand::Paste);
    }
}
