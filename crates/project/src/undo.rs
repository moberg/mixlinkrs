//! Snapshot undo stack. 80 levels; consecutive same-title pushes coalesce
//! (MixLink `mutate("Move clip", coalescing: true)`).

/// MixLink `UndoManager.levelsOfUndo`.
pub const UNDO_LEVELS: usize = 80;

#[derive(Clone, Debug)]
pub struct UndoEntry<T> {
    pub title: String,
    pub before: T,
}

/// Restores a previous `T` on undo. Coalesced titles keep the first snapshot.
#[derive(Clone, Debug)]
pub struct UndoStack<T> {
    undo: Vec<UndoEntry<T>>,
    redo: Vec<UndoEntry<T>>,
    limit: usize,
}

impl<T> Default for UndoStack<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> UndoStack<T> {
    pub fn new() -> Self {
        Self::with_limit(UNDO_LEVELS)
    }

    pub fn with_limit(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn last_title(&self) -> Option<&str> {
        self.undo.last().map(|e| e.title.as_str())
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Record `before` as the restore point. Same consecutive `title` with
    /// `coalesce` keeps the original snapshot (a 40-step clip drag is one undo).
    pub fn push(&mut self, title: impl Into<String>, before: T, coalesce: bool) {
        let title = title.into();
        if coalesce {
            if let Some(last) = self.undo.last() {
                if last.title == title {
                    self.redo.clear();
                    return;
                }
            }
        }
        self.undo.push(UndoEntry { title, before });
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
}

impl<T> UndoStack<T> {
    /// Undo: return the stored snapshot and push `current` onto redo.
    pub fn undo(&mut self, current: T) -> Option<(String, T)> {
        let entry = self.undo.pop()?;
        let title = entry.title.clone();
        self.redo.push(UndoEntry { title: entry.title, before: current });
        Some((title, entry.before))
    }

    /// Redo: return the stored snapshot and push `current` onto undo.
    pub fn redo(&mut self, current: T) -> Option<(String, T)> {
        let entry = self.redo.pop()?;
        let title = entry.title.clone();
        self.undo.push(UndoEntry { title: entry.title, before: current });
        Some((title, entry.before))
    }
}

impl<T: Clone + PartialEq> UndoStack<T> {
    /// Apply `change` to `value`. Push an undo snapshot when the value mutates.
    pub fn mutate(
        &mut self,
        title: &str,
        coalesce: bool,
        value: &mut T,
        change: impl FnOnce(&mut T),
    ) -> bool {
        let before = value.clone();
        change(value);
        if *value == before {
            return false;
        }
        self.push(title, before, coalesce);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_same_title() {
        let mut stack = UndoStack::new();
        let mut doc = 0i32;
        stack.mutate("Move clip", true, &mut doc, |d| *d = 1);
        stack.mutate("Move clip", true, &mut doc, |d| *d = 2);
        stack.mutate("Move clip", true, &mut doc, |d| *d = 40);
        assert_eq!(stack.undo_len(), 1);
        assert_eq!(stack.last_title(), Some("Move clip"));
        let (_, restored) = stack.undo(doc).unwrap();
        assert_eq!(restored, 0);
    }

    #[test]
    fn different_title_does_not_coalesce() {
        let mut stack = UndoStack::new();
        let mut doc = 0i32;
        stack.mutate("Move clip", true, &mut doc, |d| *d = 1);
        stack.mutate("Start", false, &mut doc, |d| *d = 2);
        assert_eq!(stack.undo_len(), 2);
    }

    #[test]
    fn levels_cap() {
        let mut stack = UndoStack::with_limit(2);
        stack.push("a", 1, false);
        stack.push("b", 2, false);
        stack.push("c", 3, false);
        assert_eq!(stack.undo_len(), 2);
        assert_eq!(stack.last_title(), Some("c"));
    }
}
