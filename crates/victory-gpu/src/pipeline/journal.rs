//! Pipelines whose creation ends the process (`VICTORY_PIPELINE_JOURNAL`).
//!
//! Some host drivers crash while compiling a translated shader, and the
//! fault can end the process before any handler of ours runs. Such a crash
//! cannot be caught, so it
//! is remembered instead: each pipeline is noted in `pending/` while it is
//! created and the note is removed when creation returns.
//!
//! Pipelines are created in parallel. Notes left over at the next start name
//! the pipelines that were being created when the process ended:
//!
//! - one note names the pipeline that crashed. It goes into `crashed.txt` and
//!   is refused from then on, its draws and dispatches dropped like those of
//!   any refused pipeline;
//! - several notes name suspects, of which at most one crashed. They go into
//!   `suspects.txt` and are not refused. Each is created alone (the others
//!   wait), so if it crashes again it leaves one note and is named. A suspect
//!   whose creation returns leaves the list.
//!
//! A line deleted from `crashed.txt` lets that pipeline be tried again.
//!
//! `crashed.txt` starts with the translator that was running when its
//! pipelines ended a run (`# translator <hash>`). Under another translator
//! (or in a list without that line) they may well be made now, so they are
//! not refused: each becomes a suspect, created alone, and is listed again
//! if it ends the run again.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// The line of `crashed.txt` that names the translator its pipelines
/// crashed under.
const TRANSLATOR: &str = "# translator ";

// The legacy shader crate does not export SOURCE_HASH. Bump this stamp
// whenever a change to victory-shader can change emitted DXIL.
const JOURNAL_TRANSLATOR: &str = "legacy-vcvt-f32-f64-b4149d4bda2d3183b8aa8f5545ca9ca3aabcc58f";

struct Journal {
    pending: PathBuf,
    crashed_file: PathBuf,
    suspects_file: PathBuf,
    /// Pipeline keys (`hash` or `hash+hash`) refused, and those logged.
    crashed: HashSet<String>,
    logged: Mutex<HashSet<String>>,
    /// Pipelines that were being created alongside others when a run ended.
    suspects: Mutex<HashSet<String>>,
    /// Shared while an ordinary pipeline is created, exclusive for a suspect
    /// (for at most `alone_for`).
    gate: Mutex<GateState>,
    gate_changed: Condvar,
    /// How long a suspect keeps other pipelines waiting. A suspect can take
    /// minutes to create (a large compute shader on a cold driver cache), and
    /// the title waits for the pipelines queued behind it. Past this, the
    /// others go ahead; if the run then ends, the suspect is one of several
    /// being created and stays a suspect rather than being refused.
    alone_for: Duration,
    next: AtomicU64,
}

/// Keeps the journal in `dir` (a folder of the cache).
pub(crate) fn use_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

fn journal() -> Option<&'static Journal> {
    static JOURNAL: OnceLock<Option<Journal>> = OnceLock::new();
    JOURNAL
        .get_or_init(|| {
            let enabled = std::env::var("VICTORY_PIPELINE_JOURNAL")
                .ok()
                .is_some_and(|value| value != "0");
            if !enabled {
                return None;
            }
            let Some(dir) = DIR.get() else {
                tracing::warn!("VICTORY_PIPELINE_JOURNAL: no cache folder; journal off");
                return None;
            };
            Journal::open(dir, JOURNAL_TRANSLATOR, process_running)
        })
        .as_ref()
}

/// Whether process `pid` is still running.
#[cfg(windows)]
fn process_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: the handle is checked before use and closed after.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

#[cfg(not(windows))]
fn process_running(_pid: u32) -> bool {
    false
}

fn read_list(path: &Path) -> HashSet<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn write_list<'a>(path: &Path, keys: impl IntoIterator<Item = &'a String>) {
    let mut keys: Vec<&String> = keys.into_iter().collect();
    keys.sort();
    let text: String = keys.into_iter().map(|k| format!("{k}\n")).collect();
    if let Err(e) = std::fs::write(path, text) {
        tracing::warn!(error = %e, path = %path.display(), "pipeline journal list not saved");
    }
}

impl Journal {
    /// The journal in `dir`, with the notes a run that ended while creating
    /// pipelines left sorted into `crashed.txt` or `suspects.txt`.
    /// `translator` names the translator of this run (its source hash).
    /// `running` says whether a process id is still running: notes of a
    /// running process (another instance) are left alone.
    fn open(dir: &Path, translator: &str, running: impl Fn(u32) -> bool) -> Option<Journal> {
        let pending = dir.join("pending");
        let crashed_file = dir.join("crashed.txt");
        let suspects_file = dir.join("suspects.txt");
        if let Err(e) = std::fs::create_dir_all(&pending) {
            tracing::warn!(error = %e, path = %pending.display(), "VICTORY_PIPELINE_JOURNAL: journal off");
            return None;
        }
        let mut crashed = read_list(&crashed_file);
        let mut suspects = read_list(&suspects_file);
        let listed_under = std::fs::read_to_string(&crashed_file)
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix(TRANSLATOR).map(|t| t.trim().to_owned()));
        if listed_under.as_deref() != Some(translator) {
            if !crashed.is_empty() {
                let mut retried: Vec<&String> = crashed.iter().collect();
                retried.sort();
                tracing::info!(
                    pipelines = ?retried,
                    listed_under = listed_under.as_deref().unwrap_or("unknown"),
                    translator,
                    "these host pipelines ended a run under another translator: each is tried again, alone"
                );
                suspects.extend(crashed.drain());
                write_list(&suspects_file, &suspects);
            }
            if let Err(e) = std::fs::write(&crashed_file, format!("{TRANSLATOR}{translator}\n")) {
                tracing::warn!(error = %e, path = %crashed_file.display(), "crashed pipeline list not reset");
            }
        }
        // (key, its stages and container sizes, as the note names them)
        let mut left: Vec<(String, String)> = Vec::new();
        for entry in std::fs::read_dir(&pending).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let parts: Vec<&str> = name.split('@').collect();
            // `<pid>-<n>` last: a process still running is creating this
            // pipeline now, and did not end while doing so.
            let pid = parts
                .last()
                .and_then(|t| t.split('-').next())
                .and_then(|p| p.parse::<u32>().ok());
            if pid.is_some_and(|p| p != std::process::id() && running(p)) {
                continue;
            }
            let key = parts[0].to_owned();
            let detail = if parts.len() == 3 { parts[1] } else { "" }.to_owned();
            if !crashed.contains(&key) && !left.iter().any(|(k, _)| *k == key) {
                left.push((key, detail));
            }
            let _ = std::fs::remove_file(entry.path());
        }
        match left.len() {
            0 => {}
            1 => {
                let (key, detail) = left.pop().unwrap();
                tracing::warn!(
                    pipeline = %key,
                    stages = %detail,
                    "the last run ended while creating this host pipeline; it is refused from now on, and its draws and dispatches are dropped"
                );
                suspects.remove(&key);
                write_list(&suspects_file, &suspects);
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&crashed_file);
                match file {
                    Ok(mut f) => {
                        if !detail.is_empty() {
                            let _ = writeln!(f, "# {}", detail.replace('+', " + "));
                        }
                        let _ = writeln!(f, "{key}");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, path = %crashed_file.display(), "crashed pipeline not saved")
                    }
                }
                crashed.insert(key);
            }
            _ => {
                tracing::warn!(
                    pipelines = ?left,
                    "the last run ended while creating these host pipelines together; each is created alone from now on until it succeeds"
                );
                suspects.extend(left.into_iter().map(|(k, _)| k));
                write_list(&suspects_file, &suspects);
            }
        }
        let mut listed: Vec<&String> = crashed.iter().collect();
        listed.sort();
        tracing::info!(
            refused = crashed.len(),
            suspects = suspects.len(),
            path = %crashed_file.display(),
            "pipeline journal on"
        );
        if !listed.is_empty() {
            tracing::warn!(
                pipelines = ?listed,
                "these host pipelines are refused (they ended earlier runs): every draw and dispatch that uses one is dropped; each is logged once when first skipped"
            );
        }
        Some(Journal {
            pending,
            crashed_file,
            suspects_file,
            crashed,
            logged: Mutex::new(HashSet::new()),
            suspects: Mutex::new(suspects),
            gate: Mutex::new(GateState::default()),
            gate_changed: Condvar::new(),
            alone_for: Duration::from_secs(5),
            next: AtomicU64::new(0),
        })
    }

    /// `detail` names the stages and their container sizes (`compute-2232` or
    /// `vertex-1024+pixel-2048`); it goes into the note's name, then into a comment
    /// above the pipeline's line in `crashed.txt`.
    fn enter(&self, key: &str, detail: &str) -> Option<Pending<'_>> {
        if self.crashed.contains(key) {
            let mut logged = self.logged.lock().unwrap_or_else(|e| e.into_inner());
            if logged.insert(key.to_owned()) {
                tracing::warn!(
                    pipeline = key,
                    list = %self.crashed_file.display(),
                    "host pipeline skipped: creating it ended an earlier run"
                );
            }
            return None;
        }
        let suspect = self
            .suspects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(key);
        // A thread that already holds the gate (a pipeline whose creation
        // makes another, e.g. a helper) does not take it again: taking it
        // twice on one thread deadlocks, against itself when it holds it
        // alone, or behind a suspect waiting to hold it alone.
        let gate = if HOLDS_GATE.get() {
            None
        } else {
            HOLDS_GATE.set(true);
            Some(if suspect {
                self.take_alone()
            } else {
                self.take_shared()
            })
        };
        let detail = if detail.is_empty() {
            String::new()
        } else {
            format!("{detail}@")
        };
        let path = self.pending.join(format!(
            "{key}@{detail}{}-{}",
            std::process::id(),
            self.next.fetch_add(1, Ordering::Relaxed)
        ));
        let note = match std::fs::File::create(&path) {
            Ok(_) => Some(path),
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "pipeline journal note not written");
                None
            }
        };
        Some(Pending {
            note,
            cleared: suspect.then(|| (self, key.to_owned())),
            _gate: gate,
        })
    }

    /// Waits until no suspect holds the gate alone or waits for it (or until
    /// that has lasted `alone_for`), then holds it shared.
    fn take_shared(&self) -> Gate<'_> {
        let mut st = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        while st.blocks(self.alone_for) {
            let wait = st.left(self.alone_for);
            st = self
                .gate_changed
                .wait_timeout(st, wait)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        st.shared += 1;
        Gate { journal: self, alone: false }
    }

    /// Waits until no other pipeline holds the gate, then holds it alone; if
    /// that takes longer than `alone_for`, goes ahead shared instead.
    fn take_alone(&self) -> Gate<'_> {
        let mut st = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        st.waiting_alone += 1;
        st.since.get_or_insert_with(Instant::now);
        loop {
            if !st.alone && st.shared == 0 {
                st.waiting_alone -= 1;
                st.alone = true;
                st.since = Some(Instant::now());
                return Gate { journal: self, alone: true };
            }
            if !st.blocks(self.alone_for) {
                st.waiting_alone -= 1;
                if st.waiting_alone == 0 && !st.alone {
                    st.since = None;
                }
                st.shared += 1;
                self.gate_changed.notify_all();
                return Gate { journal: self, alone: false };
            }
            let wait = st.left(self.alone_for);
            st = self
                .gate_changed
                .wait_timeout(st, wait)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn release(&self, alone: bool) {
        let mut st = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        if alone {
            st.alone = false;
            // The next suspect waiting gets its own `alone_for`.
            st.since = (st.waiting_alone > 0).then(Instant::now);
        } else {
            st.shared -= 1;
            // An ordinary pipeline returning does not restart the window:
            // otherwise a suspect waiting behind a busy queue would hold
            // every pipeline up for `alone_for` again at each return.
            if !st.alone && st.waiting_alone == 0 {
                st.since = None;
            }
        }
        self.gate_changed.notify_all();
    }

    /// A suspect holds the gate alone right now (tests).
    #[cfg(test)]
    fn held_alone(&self) -> bool {
        self.gate.lock().unwrap_or_else(|e| e.into_inner()).alone
    }

    /// `key` was created without ending the process: no longer a suspect.
    fn clear(&self, key: &str) {
        let mut suspects = self.suspects.lock().unwrap_or_else(|e| e.into_inner());
        if suspects.remove(key) {
            write_list(&self.suspects_file, suspects.iter());
        }
    }
}

#[derive(Default)]
struct GateState {
    /// Ordinary pipelines (and suspects past `alone_for`) being created.
    shared: usize,
    /// A suspect is being created alone.
    alone: bool,
    /// Suspects waiting to be created alone.
    waiting_alone: usize,
    /// Since when a suspect has held the gate alone or waited for it.
    since: Option<Instant>,
}

impl GateState {
    /// Ordinary pipelines must wait: a suspect holds the gate alone or waits
    /// for it, and has not yet for `alone_for`.
    fn blocks(&self, alone_for: Duration) -> bool {
        (self.alone || self.waiting_alone > 0)
            && self.since.is_some_and(|t| t.elapsed() < alone_for)
    }

    fn left(&self, alone_for: Duration) -> Duration {
        self.since.map_or(alone_for, |t| alone_for.saturating_sub(t.elapsed()))
            + Duration::from_millis(1)
    }
}

/// A hold on the journal's gate, given back when dropped.
struct Gate<'a> {
    journal: &'a Journal,
    alone: bool,
}

impl Drop for Gate<'_> {
    fn drop(&mut self) {
        self.journal.release(self.alone);
    }
}

/// A pipeline being created; its note is removed when this is dropped,
/// then a suspect waiting to be created alone may start.
pub(crate) struct Pending<'a> {
    note: Option<PathBuf>,
    cleared: Option<(&'a Journal, String)>,
    _gate: Option<Gate<'a>>,
}

thread_local! {
    /// This thread holds the gate (through a [`Pending`] not yet dropped).
    static HOLDS_GATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if self._gate.take().is_some() {
            HOLDS_GATE.set(false);
        }
        if let Some(path) = &self.note {
            let _ = std::fs::remove_file(path);
        }
        if let Some((j, key)) = &self.cleared {
            j.clear(key);
        }
    }
}

/// Notes that the pipeline `key` is being created, or `None` if it ended a
/// run before (it must not be created). Always `Some` with the journal off.
pub(crate) fn enter(key: &str, detail: &str) -> Option<Pending<'static>> {
    match journal() {
        Some(j) => j.enter(key, detail),
        None => Some(Pending {
            note: None,
            cleared: None,
            _gate: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The process ends while `p` is being created: its note stays. (A
    /// real crash ends the thread too, so its hold on the gate goes.)
    fn crash(p: Pending<'_>) {
        std::mem::forget(p);
        HOLDS_GATE.set(false);
    }

    #[test]
    fn a_note_left_by_a_crash_refuses_that_pipeline_from_then_on() {
        let dir = tempfile::tempdir().unwrap();
        {
            let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
            let done = j.enter("aaaaaaaaaaaaaaaa", "").unwrap();
            drop(done);
            let crashing = j.enter("bbbbbbbbbbbbbbbb+cccccccccccccccc", "").unwrap();
            // The process ends here: the note stays.
            crash(crashing);
        }
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        assert!(j.enter("bbbbbbbbbbbbbbbb+cccccccccccccccc", "").is_none());
        assert!(j.enter("aaaaaaaaaaaaaaaa", "").is_some());
        let listed = std::fs::read_to_string(dir.path().join("crashed.txt")).unwrap();
        assert_eq!(
            listed,
            "# translator t1\nbbbbbbbbbbbbbbbb+cccccccccccccccc\n"
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("pending"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn notes_left_by_pipelines_created_together_make_suspects_that_run_alone() {
        let dir = tempfile::tempdir().unwrap();
        {
            let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
            // Two pipelines in creation at once when the process ends.
            crash(j.enter("eeeeeeeeeeeeeeee", "").unwrap());
            crash(j.enter("ffffffffffffffff", "").unwrap());
        }
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("crashed.txt")).unwrap_or_default(),
            "# translator t1\n"
        );
        let suspects = std::fs::read_to_string(dir.path().join("suspects.txt")).unwrap();
        assert_eq!(suspects, "eeeeeeeeeeeeeeee\nffffffffffffffff\n");
        // A suspect is created alone: no other pipeline may hold the gate.
        let alone = j.enter("eeeeeeeeeeeeeeee", "").unwrap();
        assert!(j.held_alone());
        drop(alone);
        // Its creation returned, so it is no longer a suspect.
        let suspects = std::fs::read_to_string(dir.path().join("suspects.txt")).unwrap();
        assert_eq!(suspects, "ffffffffffffffff\n");
        // Created alone again, it crashes: now it is named.
        crash(j.enter("ffffffffffffffff", "").unwrap());
        drop(j);
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        assert!(j.enter("ffffffffffffffff", "").is_none());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("suspects.txt")).unwrap(),
            ""
        );
    }

    #[test]
    fn a_crashed_pipeline_is_listed_with_its_stages_and_sizes() {
        let dir = tempfile::tempdir().unwrap();
        {
            let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
            crash(
                j.enter(
                    "1234123412341234+5678567856785678",
                    "vertex-1024+pixel-2048",
                )
                .unwrap(),
            );
            let names: Vec<String> = std::fs::read_dir(dir.path().join("pending"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(names.len(), 1);
            assert!(
                names[0].starts_with("1234123412341234+5678567856785678@vertex-1024+pixel-2048@")
            );
        }
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        assert!(j.enter("1234123412341234+5678567856785678", "").is_none());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("crashed.txt")).unwrap(),
            "# translator t1\n# vertex-1024 + pixel-2048\n1234123412341234+5678567856785678\n"
        );
    }

    #[test]
    fn a_note_of_a_process_still_running_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending");
        std::fs::create_dir_all(&pending).unwrap();
        // Another instance (process 4242) is creating this pipeline now.
        std::fs::write(pending.join("abababababababab@compute-100@4242-7"), "").unwrap();
        let j = Journal::open(dir.path(), "t1", |p| p == 4242).unwrap();
        assert!(j.enter("abababababababab", "").is_some());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("crashed.txt")).unwrap_or_default(),
            "# translator t1\n"
        );
        assert!(pending.join("abababababababab@compute-100@4242-7").exists());
        // Once that process is gone, the note it left counts.
        drop(j);
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        assert!(j.enter("abababababababab", "").is_none());
    }

    #[test]
    fn the_running_process_check_knows_this_process() {
        assert!(process_running(std::process::id()));
    }

    #[test]
    fn ordinary_pipelines_are_created_together() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        let a = j.enter("1111111111111111", "").unwrap();
        let b = j.enter("2222222222222222", "").unwrap();
        drop((a, b));
    }

    #[test]
    fn a_pipeline_that_crashed_under_another_translator_is_tried_again_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("crashed.txt"),
            "# translator old\n# compute-3360\n9999999999999999\n",
        )
        .unwrap();
        let j = Journal::open(dir.path(), "new", |_| false).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("crashed.txt")).unwrap(),
            "# translator new\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("suspects.txt")).unwrap(),
            "9999999999999999\n"
        );
        // Tried again alone; it ends the run again, so it is listed again,
        // now under this translator.
        let alone = j.enter("9999999999999999", "compute-3360").unwrap();
        assert!(j.held_alone());
        crash(alone);
        drop(j);
        let j = Journal::open(dir.path(), "new", |_| false).unwrap();
        assert!(j.enter("9999999999999999", "").is_none());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("crashed.txt")).unwrap(),
            "# translator new\n# compute-3360\n9999999999999999\n"
        );
    }

    #[test]
    fn under_the_same_translator_a_crashed_pipeline_stays_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("crashed.txt"),
            "# translator t1\n7777777777777777\n",
        )
        .unwrap();
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        assert!(j.enter("7777777777777777", "").is_none());
    }

    #[test]
    fn a_list_without_its_translator_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("crashed.txt"), "8888888888888888\n").unwrap();
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        assert!(j.enter("8888888888888888", "").is_some());
    }

    /// A suspect keeps other pipelines waiting only for `alone_for`: one
    /// that takes minutes to create must not hold up every other pipeline
    /// (and the title waiting for them) that long.
    #[test]
    fn a_slow_suspect_holds_others_up_only_for_a_while() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("suspects.txt"), "aaaaaaaaaaaaaaaa\n").unwrap();
        let mut j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        j.alone_for = std::time::Duration::from_millis(200);
        let j = &j;
        std::thread::scope(|s| {
            let slow = j.enter("aaaaaaaaaaaaaaaa", "").unwrap();
            assert!(j.held_alone());
            let started = std::time::Instant::now();
            let other = s.spawn(move || {
                let p = j.enter("bbbbbbbbbbbbbbbb", "").unwrap();
                let waited = started.elapsed();
                drop(p);
                waited
            });
            let waited = other.join().unwrap();
            assert!(waited >= std::time::Duration::from_millis(150), "{waited:?}");
            assert!(waited < std::time::Duration::from_secs(3), "{waited:?}");
            drop(slow);
        });
        // Once it returned, nothing holds the gate.
        assert!(!j.held_alone());
        drop(j.enter("cccccccccccccccc", "").unwrap());
    }

    /// A suspect waiting while ordinary pipelines keep returning holds them
    /// up once for `alone_for`, not again at each return.
    #[test]
    fn a_waiting_suspect_does_not_restart_its_window_as_others_return() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("suspects.txt"), "aaaaaaaaaaaaaaaa\n").unwrap();
        let mut j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        j.alone_for = Duration::from_millis(300);
        let j = &j;
        std::thread::scope(|s| {
            // Six ordinary pipelines are being created, each on its thread.
            let mut busy = Vec::new();
            for _ in 0..6 {
                let (go, wait) = std::sync::mpsc::channel::<()>();
                let (held, ready) = std::sync::mpsc::channel::<()>();
                s.spawn(move || {
                    let p = j.enter("bbbbbbbbbbbbbbbb", "").unwrap();
                    held.send(()).unwrap();
                    let _ = wait.recv();
                    drop(p);
                });
                ready.recv().unwrap();
                busy.push(go);
            }
            // The suspect waits for them.
            let suspect = s.spawn(move || drop(j.enter("aaaaaaaaaaaaaaaa", "").unwrap()));
            std::thread::sleep(Duration::from_millis(20));
            // A new pipeline comes, and the six return one by one.
            let started = Instant::now();
            let new = s.spawn(move || {
                drop(j.enter("cccccccccccccccc", "").unwrap());
                started.elapsed()
            });
            for go in busy {
                std::thread::sleep(Duration::from_millis(100));
                let _ = go.send(());
            }
            let waited = new.join().unwrap();
            assert!(waited < Duration::from_millis(500), "{waited:?}");
            suspect.join().unwrap();
        });
    }

    /// A pipeline made while this thread already holds the gate (a helper
    /// made inside another's creation) does not take it again: alone (a
    /// suspect) or shared behind a waiting suspect, that deadlocked.
    #[test]
    fn a_nested_pipeline_does_not_take_the_gate_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("suspects.txt"), "aaaaaaaaaaaaaaaa\n").unwrap();
        let path = dir.path().to_path_buf();
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let j = Journal::open(&path, "t1", |_| false).unwrap();
            // A suspect holds the gate alone and makes a helper pipeline.
            let outer = j.enter("aaaaaaaaaaaaaaaa", "").unwrap();
            let inner = j.enter("bbbbbbbbbbbbbbbb", "").unwrap();
            drop(inner);
            drop(outer);
            // And the gate is free again for the next pipeline.
            drop(j.enter("cccccccccccccccc", "").unwrap());
            let _ = done.send(());
        });
        assert!(
            finished.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "a nested pipeline deadlocked on the gate"
        );
    }

    #[test]
    fn a_line_removed_from_the_list_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("crashed.txt"), "# tried again\n").unwrap();
        let j = Journal::open(dir.path(), "t1", |_| false).unwrap();
        assert!(j.enter("dddddddddddddddd", "").is_some());
    }
}
