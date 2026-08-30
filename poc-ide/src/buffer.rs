//! `OpenBuffer` / `BufferMap` Entity + Identity, `Selection` and `DirtyFlag`.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ropey::Rope;

use crate::error::IdeError;
use crate::ports::FsPort;

/// Ordered char-offset range. Constructor keeps `start <= end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    start: usize,
    end: usize,
}

impl Selection {
    pub fn new(start: usize, end: usize) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    pub fn collapsed(offset: usize) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::collapsed(0)
    }
}

/// Char offsets from the editor view. Maps to [`Selection`] without egui types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CursorOffsets {
    start: usize,
    end: usize,
}

impl CursorOffsets {
    pub fn new(start: usize, end: usize) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    pub fn collapsed(offset: usize) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    pub fn start(self) -> usize {
        self.start
    }

    pub fn end(self) -> usize {
        self.end
    }

    pub fn to_selection(self) -> Selection {
        Selection::new(self.start, self.end)
    }

    /// Write the visible caret/range onto the buffer. Does not dirty the rope.
    pub fn apply(self, buffer: &mut OpenBuffer) {
        buffer.set_selection(self.to_selection());
    }
}

/// Edit sets dirty; successful save clears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyFlag {
    dirty: bool,
}

impl DirtyFlag {
    pub fn clean() -> Self {
        Self { dirty: false }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark(&mut self) {
        self.dirty = true;
    }

    pub fn clear(&mut self) {
        self.dirty = false;
    }
}

/// One open file. Rope is the source of truth.
#[derive(Clone, Debug)]
pub struct OpenBuffer {
    path: PathBuf,
    rope: Rope,
    selection: Selection,
    dirty: DirtyFlag,
}

impl OpenBuffer {
    pub fn load(path: impl AsRef<Path>, fs: &(impl FsPort + ?Sized)) -> Result<Self, IdeError> {
        let path = fs.canonicalize(path.as_ref())?;
        let bytes = fs.read(&path)?;
        let text = String::from_utf8(bytes).map_err(|_| IdeError::InvalidUtf8(path.clone()))?;
        Ok(Self {
            path,
            rope: Rope::from_str(&text),
            selection: Selection::collapsed(0),
            dirty: DirtyFlag::clean(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn set_selection(&mut self, selection: Selection) {
        self.selection = self.clamp_selection(selection);
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.is_dirty()
    }

    pub fn dirty_flag(&self) -> DirtyFlag {
        self.dirty
    }

    pub fn selected_text(&self) -> String {
        let sel = self.clamp_selection(self.selection);
        self.rope.slice(sel.start..sel.end).to_string()
    }

    pub fn save(&mut self, fs: &mut (impl FsPort + ?Sized)) -> Result<(), IdeError> {
        fs.write(&self.path, self.rope.to_string().as_bytes())?;
        self.dirty.clear();
        Ok(())
    }

    /// Replace the rope from `FsPort` and clear dirty. Used by `ConflictChoice::LoadDisk`.
    pub fn reload_from(&mut self, fs: &(impl FsPort + ?Sized)) -> Result<(), IdeError> {
        let bytes = fs.read(&self.path)?;
        let text =
            String::from_utf8(bytes).map_err(|_| IdeError::InvalidUtf8(self.path.clone()))?;
        self.rope = Rope::from_str(&text);
        self.dirty.clear();
        self.selection = self.clamp_selection(self.selection);
        Ok(())
    }

    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        let sel = self.clamp_selection(self.selection);
        let changed = sel.start != sel.end || !text.is_empty();
        if sel.start != sel.end {
            self.rope.remove(sel.start..sel.end);
        }
        if !text.is_empty() {
            self.rope.insert(sel.start, text);
        }
        let cursor = sel.start + text.chars().count();
        self.selection = Selection::collapsed(cursor);
        if changed {
            self.dirty.mark();
        }
        changed
    }

    pub(crate) fn delete_range_or_forward(&mut self) -> bool {
        let sel = self.clamp_selection(self.selection);
        if sel.start != sel.end {
            self.rope.remove(sel.start..sel.end);
            self.selection = Selection::collapsed(sel.start);
            self.dirty.mark();
            return true;
        }
        let len = self.rope.len_chars();
        if sel.start < len {
            self.rope.remove(sel.start..sel.start + 1);
            self.dirty.mark();
            return true;
        }
        false
    }

    pub(crate) fn delete_selection_only(&mut self) -> bool {
        let sel = self.clamp_selection(self.selection);
        if sel.start == sel.end {
            return false;
        }
        self.rope.remove(sel.start..sel.end);
        self.selection = Selection::collapsed(sel.start);
        self.dirty.mark();
        true
    }

    fn clamp_selection(&self, selection: Selection) -> Selection {
        let len = self.rope.len_chars();
        Selection::new(selection.start.min(len), selection.end.min(len))
    }
}

/// One [`OpenBuffer`] per canonical path.
#[derive(Clone, Debug, Default)]
pub struct BufferMap {
    buffers: BTreeMap<PathBuf, OpenBuffer>,
}

impl BufferMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(
        &mut self,
        path: impl AsRef<Path>,
        fs: &(impl FsPort + ?Sized),
    ) -> Result<&mut OpenBuffer, IdeError> {
        let canon = fs.canonicalize(path.as_ref())?;
        match self.buffers.entry(canon) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let buffer = OpenBuffer::load(entry.key(), fs)?;
                Ok(entry.insert(buffer))
            }
        }
    }

    pub fn get(&self, path: impl AsRef<Path>) -> Option<&OpenBuffer> {
        self.buffers.get(path.as_ref())
    }

    pub fn get_mut(&mut self, path: impl AsRef<Path>) -> Option<&mut OpenBuffer> {
        self.buffers.get_mut(path.as_ref())
    }

    pub fn contains(&self, path: impl AsRef<Path>) -> bool {
        self.buffers.contains_key(path.as_ref())
    }

    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }

    pub fn close(&mut self, path: impl AsRef<Path>) -> Option<OpenBuffer> {
        self.buffers.remove(path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::MemFs;

    fn sample_fs() -> MemFs {
        let mut fs = MemFs::new();
        fs.add_file("/ws/src/lib.rs", "fn x() {}\n").unwrap();
        fs.add_file("/ws/src/café.rs", "let café = 1;\n").unwrap();
        fs.add_file("/ws/bin.dat", [0xff, 0xfe, 0xfd]).unwrap();
        fs
    }

    #[test]
    fn selection_value_object_start_le_end() {
        let ordered = Selection::new(1, 4);
        assert_eq!(ordered.start(), 1);
        assert_eq!(ordered.end(), 4);
        assert!(!ordered.is_empty());
        assert_eq!(ordered.len(), 3);
        let swapped = Selection::new(4, 1);
        assert_eq!(swapped.start(), 1);
        assert_eq!(swapped.end(), 4);
        assert_eq!(swapped, ordered);
        let empty = Selection::collapsed(2);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.start(), 2);
        assert_eq!(empty.end(), 2);
        assert_eq!(Selection::default(), Selection::collapsed(0));
        assert!(Selection::new(0, 0).is_empty());
        assert_eq!(Selection::new(7, 7).len(), 0);
    }

    #[test]
    fn cursor_offsets_map_to_selection_and_lsp_position_not_origin() {
        use crate::lsp::position_at;

        let text = "fn alpha() {\n    beta();\n}\n";
        let y = "fn alpha() {\n    ".chars().count();
        assert_eq!(text.chars().nth(y), Some('b'));
        let offsets = CursorOffsets::new(y, y + 4);
        assert_eq!(offsets.start(), y);
        assert_eq!(offsets.end(), y + 4);
        assert_eq!(offsets, CursorOffsets::new(y + 4, y));
        assert_eq!(
            CursorOffsets::collapsed(y).to_selection(),
            Selection::collapsed(y)
        );
        let sel = offsets.to_selection();
        assert_eq!(sel, Selection::new(y, y + 4));
        assert_ne!(sel, Selection::collapsed(0));
        let (line, character) = position_at(text, sel.start());
        assert_eq!((line, character), (1, 4));
        assert_ne!((line, character), (0, 0));

        let mut fs = MemFs::new();
        fs.add_file("/ws/src/lib.rs", text).unwrap();
        let mut buf = OpenBuffer::load("/ws/src/lib.rs", &fs).unwrap();
        assert_eq!(buf.selection(), Selection::collapsed(0));
        offsets.apply(&mut buf);
        assert_eq!(buf.selection(), sel);
        assert!(!buf.is_dirty());
        let (line, character) = position_at(&buf.text(), buf.selection().start());
        assert_eq!((line, character), (1, 4));

        let cafe = "let café = 1;\n";
        let e_acute = "let caf".chars().count();
        let cafe_sel = CursorOffsets::collapsed(e_acute).to_selection();
        assert_eq!(cafe_sel, Selection::collapsed(e_acute));
        assert_ne!(position_at(cafe, cafe_sel.start()), (0, 0));
        assert_eq!(position_at(cafe, cafe_sel.start()), (0, 7));
    }

    #[test]
    fn dirty_flag_value_object_edit_sets_save_clears() {
        let mut flag = DirtyFlag::clean();
        assert!(!flag.is_dirty());
        assert_eq!(flag, DirtyFlag::clean());
        flag.mark();
        assert!(flag.is_dirty());
        flag.mark();
        assert!(flag.is_dirty());
        flag.clear();
        assert!(!flag.is_dirty());
        flag.clear();
        assert!(!flag.is_dirty());
    }

    #[test]
    fn open_buffer_entity_rope_is_source_of_truth() {
        let fs = sample_fs();
        let buf = OpenBuffer::load("/ws/src/lib.rs", &fs).unwrap();
        assert_eq!(buf.path(), Path::new("/ws/src/lib.rs"));
        assert_eq!(buf.text(), "fn x() {}\n");
        assert_eq!(buf.len_chars(), 10);
        assert!(!buf.is_dirty());
        assert!(!buf.dirty_flag().is_dirty());
        assert_eq!(buf.dirty_flag(), DirtyFlag::clean());
        assert!(buf.selection().is_empty());
        assert_eq!(buf.selected_text(), "");
        assert!(OpenBuffer::load("/missing.rs", &fs)
            .unwrap_err()
            .is_not_found());
        assert!(OpenBuffer::load("rel.rs", &fs)
            .unwrap_err()
            .is_not_absolute());
    }

    #[test]
    fn open_buffer_entity_invalid_utf8_is_domain_result() {
        let fs = sample_fs();
        let err = OpenBuffer::load("/ws/bin.dat", &fs).unwrap_err();
        assert!(err.is_invalid_utf8());
        assert!(err.to_string().contains("invalid UTF-8"));
    }

    #[test]
    fn open_buffer_entity_selection_clamps_to_rope() {
        let fs = sample_fs();
        let mut buf = OpenBuffer::load("/ws/src/lib.rs", &fs).unwrap();
        buf.set_selection(Selection::new(100, 200));
        assert_eq!(buf.selection(), Selection::collapsed(buf.len_chars()));
        buf.set_selection(Selection::new(2, 100));
        assert_eq!(buf.selection(), Selection::new(2, buf.len_chars()));
        buf.set_selection(Selection::new(3, 5));
        assert_eq!(buf.selected_text(), "x(");
        let cafe = OpenBuffer::load("/ws/src/café.rs", &fs).unwrap();
        assert_eq!(cafe.len_chars(), "let café = 1;\n".chars().count());
    }

    #[test]
    fn open_buffer_entity_save_clears_dirty_only_on_success() {
        let mut fs = sample_fs();
        let mut buf = OpenBuffer::load("/ws/src/lib.rs", &fs).unwrap();
        assert!(buf.insert_text("x"));
        assert!(buf.is_dirty());
        buf.save(&mut fs).unwrap();
        assert!(!buf.is_dirty());
        assert_eq!(
            fs.read(Path::new("/ws/src/lib.rs")).unwrap(),
            b"xfn x() {}\n"
        );

        buf.insert_text("!");
        assert!(buf.is_dirty());
        assert!(buf.save(&mut fs).is_ok());
        assert!(!buf.is_dirty());
    }

    #[test]
    fn open_buffer_entity_save_to_directory_keeps_dirty() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/a.rs", b"hi").unwrap();
        let mut buf = OpenBuffer::load("/ws/a.rs", &fs).unwrap();
        buf.insert_text("x");
        assert!(buf.is_dirty());
        fs.add_dir("/ws/a.rs").unwrap();
        let err = buf.save(&mut fs).unwrap_err();
        assert!(err.is_directory());
        assert!(buf.is_dirty());
        assert_eq!(buf.text(), "xhi");
    }

    #[test]
    fn open_buffer_entity_reload_from_replaces_rope_and_clears_dirty() {
        let mut fs = sample_fs();
        let mut buf = OpenBuffer::load("/ws/src/lib.rs", &fs).unwrap();
        assert!(buf.insert_text("EDIT"));
        buf.set_selection(Selection::new(0, 100));
        assert!(buf.is_dirty());
        fs.write(Path::new("/ws/src/lib.rs"), b"from disk\n")
            .unwrap();
        buf.reload_from(&fs).unwrap();
        assert_eq!(buf.text(), "from disk\n");
        assert!(!buf.is_dirty());
        assert_eq!(buf.selection(), Selection::new(0, buf.len_chars()));

        let mut dirty = OpenBuffer::load("/ws/src/lib.rs", &fs).unwrap();
        dirty.insert_text("x");
        fs.write(Path::new("/ws/src/lib.rs"), &[0xff, 0xfe])
            .unwrap();
        assert!(dirty.reload_from(&fs).unwrap_err().is_invalid_utf8());
        assert_eq!(dirty.text(), "xfrom disk\n");
        assert!(dirty.is_dirty());

        let mut missing = OpenBuffer::load("/ws/src/café.rs", &fs).unwrap();
        missing.insert_text("keep");
        assert!(missing
            .reload_from(&MemFs::new())
            .unwrap_err()
            .is_not_found());
        assert_eq!(missing.text(), "keeplet café = 1;\n");
        assert!(missing.is_dirty());
    }

    #[test]
    fn buffer_map_identity_one_buffer_per_canonical_path() {
        let fs = sample_fs();
        let mut map = BufferMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(BufferMap::default().len(), 0);
        assert!(map.get("/ws/src/lib.rs").is_none());
        assert!(!map.contains("/ws/src/lib.rs"));
        assert!(map.close("/ws/src/lib.rs").is_none());

        map.open("/ws/src/lib.rs", &fs).unwrap().insert_text("A");
        assert_eq!(map.len(), 1);
        assert!(!map.is_empty());
        assert!(map.contains("/ws/src/lib.rs"));
        assert!(map.get("/ws/src/lib.rs").unwrap().dirty_flag().is_dirty());
        assert_eq!(map.get("/ws/src/lib.rs").unwrap().text(), "Afn x() {}\n");
        assert!(map.get("/ws/src/lib.rs").unwrap().is_dirty());

        map.open("/ws/src/lib.rs", &fs).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("/ws/src/lib.rs").unwrap().text(), "Afn x() {}\n");
        assert!(map.get("/ws/src/lib.rs").unwrap().is_dirty());

        map.open("/ws/src/café.rs", &fs).unwrap();
        assert_eq!(map.len(), 2);
        assert!(!map.get("/ws/src/café.rs").unwrap().is_dirty());

        let closed = map.close("/ws/src/lib.rs").unwrap();
        assert_eq!(closed.text(), "Afn x() {}\n");
        assert_eq!(map.len(), 1);
        assert!(!map.contains("/ws/src/lib.rs"));
        assert!(map.get_mut("/ws/src/café.rs").is_some());
        assert!(map.get_mut("/missing").is_none());
        assert!(map.open("/missing.rs", &fs).unwrap_err().is_not_found());
    }
}
