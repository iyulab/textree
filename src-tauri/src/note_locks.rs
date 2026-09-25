//! Writes to one note happen one at a time.
//!
//! Saves run off the main thread, so two of them can be in flight together. For two different
//! notes that is the point — a folder that stops answering for one note must not hold up another.
//! For the same note it would be a loss: a save that stalls after its check and lands late would
//! overwrite a newer one that went ahead meanwhile. A write can't be called back once the disk has
//! it, so the only safe order is the one enforced here: the next save to a note starts after the
//! previous one has actually finished, and checks the note against what that one left.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

/// The notes a save is running for right now.
#[derive(Default)]
pub struct NoteLocks {
    busy: Mutex<HashSet<PathBuf>>,
    freed: Condvar,
}

/// Holds one note's turn. Dropping it lets the next save to that note start.
pub struct Turn<'a> {
    locks: &'a NoteLocks,
    key: PathBuf,
}

impl NoteLocks {
    /// Waits until no other save to `path` is running, then holds its turn.
    pub fn turn(&self, path: &Path) -> Turn<'_> {
        let key = key(path);
        let mut busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        while busy.contains(&key) {
            busy = self.freed.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        busy.insert(key.clone());
        Turn { locks: self, key }
    }

    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.busy.lock().unwrap().len()
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut busy = self.locks.busy.lock().unwrap_or_else(|e| e.into_inner());
        busy.remove(&self.key);
        self.locks.freed.notify_all();
    }
}

/// One key per file: Windows paths differ in case and separator without naming a different file.
fn key(path: &Path) -> PathBuf {
    let components: PathBuf = path.components().collect();
    if cfg!(windows) {
        PathBuf::from(components.to_string_lossy().to_lowercase())
    } else {
        components
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn a_second_turn_on_the_same_note_waits_for_the_first() {
        let locks = Arc::new(NoteLocks::default());
        let first = locks.turn(Path::new("/v/a.md"));

        let (done, finished) = mpsc::channel();
        let waiter = {
            let locks = Arc::clone(&locks);
            std::thread::spawn(move || {
                let _t = locks.turn(Path::new("/v/a.md"));
                done.send(()).unwrap();
            })
        };
        assert!(
            finished.recv_timeout(Duration::from_millis(200)).is_err(),
            "the second save to a note must not start while the first runs"
        );
        drop(first);
        finished.recv_timeout(Duration::from_secs(5)).expect("it starts once the first ends");
        waiter.join().unwrap();
    }

    #[test]
    fn another_note_does_not_wait() {
        let locks = NoteLocks::default();
        let _a = locks.turn(Path::new("/v/a.md"));
        let _b = locks.turn(Path::new("/v/b.md")); // would deadlock this thread if it waited
    }

    #[cfg(windows)]
    #[test]
    fn spellings_of_one_file_share_a_turn() {
        assert_eq!(key(Path::new(r"C:\V\A.md")), key(Path::new("c:/v/a.md")));
    }

    #[test]
    fn a_note_nobody_is_saving_is_forgotten() {
        let locks = NoteLocks::default();
        drop(locks.turn(Path::new("/v/a.md")));
        assert_eq!(locks.tracked(), 0);
    }
}
