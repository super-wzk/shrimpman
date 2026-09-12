//! Per-file editing history. Expanded inspector details are not edits.

use crate::inspect::Document;
use std::{collections::HashSet, sync::Arc};

// These bound the undo cache, not resource input sizes. Keep the adjacent undo
// and redo states even when a single large resource exceeds the byte budget.
const HISTORY_BYTES: usize = 128 * 1024 * 1024;
const HISTORY_STEPS: usize = 128;

pub(crate) struct Session {
    pub document: Arc<Document>,
    undo: Vec<Arc<Document>>,
    redo: Vec<Arc<Document>>,
    saved: Arc<[u8]>,
    dirty: bool,
}

impl Session {
    pub fn new(document: Arc<Document>) -> Self {
        Self {
            saved: document.buffers[0].clone(),
            document,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
        }
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn apply(&mut self, document: Arc<Document>) {
        if self.document.buffers[0] == document.buffers[0] {
            return;
        }
        self.undo
            .push(std::mem::replace(&mut self.document, document));
        self.redo.clear();
        self.trim_history(HISTORY_BYTES);
        self.update_dirty();
    }

    pub fn undo(&mut self) {
        if let Some(document) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.document, document));
            self.trim_history(HISTORY_BYTES);
            self.update_dirty();
        }
    }

    pub fn redo(&mut self) {
        if let Some(document) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.document, document));
            self.trim_history(HISTORY_BYTES);
            self.update_dirty();
        }
    }

    /// Adopt packing repairs only while the requested revision is still current.
    /// Always mark the written snapshot, even when newer edits must be retained.
    pub fn saved(&mut self, requested: &Document, packed: &Arc<Document>) -> bool {
        self.saved = packed.buffers[0].clone();
        let updated = self.document.buffers[0] == requested.buffers[0]
            && self.document.buffers[0] != self.saved;
        if updated {
            self.apply(packed.clone());
        } else {
            self.update_dirty();
        }
        updated
    }

    fn history_bytes(&self) -> usize {
        let mut seen = HashSet::new();
        self.undo
            .iter()
            .chain(&self.redo)
            .flat_map(|document| &document.buffers)
            .filter(|buffer| seen.insert(buffer.as_ptr()))
            .fold(0usize, |total, buffer| total.saturating_add(buffer.len()))
    }

    fn trim_history(&mut self, budget: usize) {
        while self.undo.len() + self.redo.len() > HISTORY_STEPS || self.history_bytes() > budget {
            // Vec order is farthest to nearest on each side of the cursor.
            // Preserve the immediate inverse operation in both directions.
            if self.undo.len() > 1 {
                self.undo.remove(0);
            } else if self.redo.len() > 1 {
                self.redo.remove(0);
            } else {
                break;
            }
        }
    }

    fn update_dirty(&mut self) {
        self.dirty = !Arc::ptr_eq(&self.document.buffers[0], &self.saved)
            && self.document.buffers[0] != self.saved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::inspect;

    fn layered(byte: u8, shared: Arc<[u8]>) -> Arc<Document> {
        let mut document = inspect("test.bin", Arc::from([byte]));
        document.buffers.push(shared);
        Arc::new(document)
    }

    #[test]
    fn history_budget_counts_decoded_layers_and_keeps_the_nearest_revisions() {
        let document = |byte| layered(byte, vec![byte; 9].into());
        let mut session = Session::new(document(0));
        for byte in 1..=4 {
            session.apply(document(byte));
        }
        session.trim_history(20);
        assert_eq!(session.undo.len(), 2);
        assert_eq!(session.history_bytes(), 20);
        session.undo();
        assert_eq!(session.document.buffers[0].as_ref(), &[3]);
        session.undo();
        assert_eq!(session.document.buffers[0].as_ref(), &[2]);
        assert!(!session.can_undo());
        session.redo();
        session.redo();
        assert_eq!(session.document.buffers[0].as_ref(), &[4]);
    }

    #[test]
    fn oversized_snapshots_keep_one_step_and_shared_buffers_count_once() {
        let shared: Arc<[u8]> = vec![0; 9].into();
        let mut session = Session::new(layered(0, shared.clone()));
        for byte in 1..=3 {
            session.apply(layered(byte, shared.clone()));
        }
        assert_eq!(session.history_bytes(), 12);
        session.trim_history(12);
        assert_eq!(session.undo.len(), 3);
        session.trim_history(1);
        assert_eq!(session.undo.len(), 1);
        session.undo();
        session.trim_history(1);
        assert!(session.can_redo());
        session.redo();
        assert_eq!(session.document.buffers[0].as_ref(), &[3]);
    }

    #[test]
    fn small_resources_still_have_a_bounded_number_of_history_records() {
        let document = |byte| Arc::new(inspect("test.bin", Arc::from([byte])));
        let mut session = Session::new(document(0));
        for byte in 1..=130 {
            session.apply(document(byte));
        }
        assert_eq!(session.undo.len(), HISTORY_STEPS);
        for _ in 0..HISTORY_STEPS {
            session.undo();
        }
        assert_eq!(session.document.buffers[0].as_ref(), &[2]);
    }

    #[test]
    fn undo_redo_and_saving_an_older_revision_keep_dirty_state_correct() {
        let document = |byte| Arc::new(inspect("test.bin", Arc::from([byte])));
        let mut session = Session::new(document(1));
        session.apply(document(2));
        let saved = session.document.clone();
        session.apply(document(3));
        assert!(!session.saved(&saved, &saved));
        assert!(session.dirty());
        session.undo();
        assert!(!session.dirty());
        session.undo();
        assert!(session.dirty());
        session.redo();
        assert!(!session.dirty());
        session.apply(document(4));
        assert!(!session.can_redo());
    }

    #[test]
    fn packing_repairs_update_the_current_document_and_remain_undoable() {
        let requested = Arc::new(inspect("file.bin", Arc::from([1, 2])));
        let packed = Arc::new(inspect("file.bin", Arc::from([3, 2])));
        let mut session = Session::new(requested.clone());
        // Expanding inspector details may clone the Document without editing bytes.
        session.document = Arc::new((*requested).clone());
        assert!(session.saved(&requested, &packed));
        assert!(Arc::ptr_eq(&session.document, &packed));
        assert!(!session.dirty());
        session.undo();
        assert_eq!(session.document.buffers[0].as_ref(), &[1, 2]);
        assert!(session.dirty());
        session.redo();
        assert!(Arc::ptr_eq(&session.document, &packed));
        assert!(!session.dirty());
        assert!(!session.saved(&packed, &packed));
    }

    #[test]
    fn packing_repairs_do_not_replace_a_newer_edited_document() {
        let requested = Arc::new(inspect("file.bin", Arc::from([1, 2])));
        let packed = Arc::new(inspect("file.bin", Arc::from([3, 2])));
        let newer = Arc::new(inspect("file.bin", Arc::from([3, 4])));
        let mut session = Session::new(requested.clone());
        session.apply(packed.clone());
        session.apply(newer.clone());
        assert!(!session.saved(&requested, &packed));
        assert!(Arc::ptr_eq(&session.document, &newer));
        assert!(session.dirty());
        session.undo();
        assert!(Arc::ptr_eq(&session.document, &packed));
        assert!(!session.dirty());
    }
}
