//! Edit > Undo for what's done to the library (Notes: Undo Delete Note, Undo Move Note, Undo Pin
//! Note, Undo New Folder, Undo Delete Folder), apart from the editor's own undo of typing: ⌘Z /
//! ⇧⌘Z take these while the note isn't being edited. Each action records what it changed, and
//! undoing puts it back (deleted notes out of Recently Deleted into their folders, pins as they
//! were, a deleted folder with its subfolders and notes).

use crate::app::App;
use nt_core::model::Folder;

#[derive(Clone, Debug)]
pub enum Op {
    /// notes sent to Recently Deleted: (id, the folder it was in, pinned)
    Delete(Vec<(String, String, bool)>),
    /// notes moved: (id, from, to)
    Move(Vec<(String, String, String)>),
    /// notes pinned or unpinned: (id, pinned before)
    Pin(Vec<(String, bool)>),
    /// a folder made (its id now, its name, parent and account, to make it again)
    NewFolder { id: String, name: String, parent: Option<String>, account: String },
    /// a folder deleted: it and its subfolders (parents first), and their notes (id, folder, pinned)
    DeleteFolder { folders: Vec<Folder>, notes: Vec<(String, String, bool)> },
}

impl Op {
    /// What Edit > Undo / Redo calls it ("Delete Note", "Move Notes", …).
    pub fn label(&self) -> String {
        let n = |k: usize, one: &str, many: &str| if k == 1 { one.to_string() } else { many.to_string() };
        match self {
            Op::Delete(v) => n(v.len(), "Delete Note", "Delete Notes"),
            Op::Move(v) => n(v.len(), "Move Note", "Move Notes"),
            Op::Pin(v) => {
                let pinning = v.iter().any(|(_, was)| !was);
                let w = if pinning { "Pin" } else { "Unpin" };
                format!("{w} {}", n(v.len(), "Note", "Notes"))
            }
            Op::NewFolder { .. } => "New Folder".into(),
            Op::DeleteFolder { .. } => "Delete Folder".into(),
        }
    }
}

#[derive(Default)]
pub struct Undo {
    done: Vec<Op>,
    undone: Vec<Op>,
}

impl Undo {
    /// A new action: undoable, and what was undone before can't be redone any more.
    pub fn record(&mut self, op: Op) {
        self.done.push(op);
        self.undone.clear();
        if self.done.len() > 100 {
            self.done.remove(0);
        }
    }

    pub fn undo_label(&self) -> Option<String> {
        self.done.last().map(Op::label)
    }

    pub fn redo_label(&self) -> Option<String> {
        self.undone.last().map(Op::label)
    }
}

/// Edit > Undo …: the last action put back. Returns whether there was one.
pub fn undo(app: &mut App) -> bool {
    let Some(op) = app.undo.done.pop() else { return false };
    let back = apply(app, op, true);
    app.undo.undone.push(back);
    true
}

/// Edit > Redo …: the last undone action done again.
pub fn redo(app: &mut App) -> bool {
    let Some(op) = app.undo.undone.pop() else { return false };
    let again = apply(app, op, false);
    app.undo.done.push(again);
    true
}

/// Undo (`back`) or redo `op`; returns it as it now stands (folders made again have new ids).
fn apply(app: &mut App, op: Op, back: bool) -> Op {
    match op {
        Op::Delete(notes) => {
            if back {
                for (id, folder, pinned) in &notes {
                    app.restore_note(id, folder, *pinned);
                }
                // (they come back selected, as they were)
                let ids: Vec<String> = notes.iter().map(|n| n.0.clone()).collect();
                app.select_these(&ids);
            } else {
                for (id, ..) in &notes {
                    app.trash_note(id);
                }
            }
            Op::Delete(notes)
        }
        Op::Move(moves) => {
            for (id, from, to) in &moves {
                app.move_note_quiet(id, if back { from } else { to });
            }
            let ids: Vec<String> = moves.iter().map(|m| m.0.clone()).collect();
            app.select_these(&ids);
            Op::Move(moves)
        }
        Op::Pin(notes) => {
            for (id, was) in &notes {
                app.set_pin(id, if back { *was } else { !*was });
            }
            Op::Pin(notes)
        }
        Op::NewFolder { id, name, parent, account } => {
            if back {
                app.delete_folder_quiet(&id);
                Op::NewFolder { id, name, parent, account }
            } else {
                let made = app.make_folder(&name, parent.as_deref(), &account).unwrap_or(id);
                Op::NewFolder { id: made, name, parent, account }
            }
        }
        Op::DeleteFolder { folders, notes } => {
            if back {
                // the folders again (new ids for them, parents first), their notes back into them
                let mut ids: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                let mut made = Vec::new();
                for f in &folders {
                    let parent = f.parent.as_ref().map(|p| ids.get(p).cloned().unwrap_or_else(|| p.clone()));
                    if let Some(new) = app.make_folder(&f.name, parent.as_deref(), &f.account) {
                        ids.insert(f.id.clone(), new.clone());
                        made.push(Folder { id: new, parent, ..f.clone() });
                    }
                }
                let notes: Vec<(String, String, bool)> = notes.into_iter().map(|(id, folder, pinned)| (id, ids.get(&folder).cloned().unwrap_or(folder), pinned)).collect();
                for (id, folder, pinned) in &notes {
                    app.restore_note(id, folder, *pinned);
                }
                if let Some(top) = made.first() {
                    app.go(crate::app::Place::Folder(top.id.clone()));
                }
                Op::DeleteFolder { folders: made, notes }
            } else {
                if let Some(top) = folders.first() {
                    app.delete_folder_quiet(&top.id);
                }
                Op::DeleteFolder { folders, notes }
            }
        }
    }
}
