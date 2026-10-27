//! Move Up / Move Down, materialised as renames (md.win `Book/RenumberPlan.cs`, Swift
//! `BookLibrary.renumberPlan` and `applyRenames`).
//!
//! The sibling group's on-disk names in displayed order plus the requested move in; the renames
//! that spell the new order out on disk out. Every sibling ends up with a zero-padded two-digit
//! prefix — unprefixed names gain one, a loose "2. " is normalised, an identical-stem swap trades
//! names — so the order lives in the names and needs no sidecar. Pure planning; the disk work is
//! [`apply`], two-phase so plans whose targets are also sources ("01-a" ↔ "02-a") never collide
//! midway.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::article_file_system::with_path;
use super::{book_naming, book_paths};

/// One rename inside a sibling group: on-disk name before and after.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenamePair {
    pub from: String,
    pub to: String,
}

impl RenamePair {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        RenamePair { from: from.into(), to: to.into() }
    }
}

/// One staged step of a two-phase rename: the item goes to `temp` first, then to `target`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StagedRename {
    pub original: String,
    pub temp: String,
    pub target: String,
}

/// The one file operation a rename plan needs; fails so the executor can roll back. (Swift's
/// `moveItem`; `move` is a Rust keyword.)
pub trait BookFileMover {
    fn move_item(&self, from: &str, to: &str) -> io::Result<()>;
}

/// [`BookFileMover`] over `std::fs` — files and folders alike.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalBookFileMover;

impl BookFileMover for LocalBookFileMover {
    fn move_item(&self, from: &str, to: &str) -> io::Result<()> {
        move_no_replace(from, to)
    }
}

/// `rename(2)` that refuses an existing destination.
///
/// LINUX DECISION: `rename(2)` silently replaces an existing file (and an empty folder), where
/// Foundation's `moveItem` and .NET's `File.Move` / `Directory.Move` fail. A move that could eat a
/// sibling is never acceptable in a book, so the destination is checked first (without following a
/// link: a dangling link is still a name that is taken). The check and the rename are two steps —
/// `renameat2(RENAME_NOREPLACE)` would close the gap but needs `libc` and `unsafe`, which md-core
/// does not take. On a case-insensitive volume (vfat, exFAT, ext4 with casefold) a case-only rename
/// finds its own source here and is refused, which is why `book_folder::rename_item` routes those
/// through a staging name, as md.win does for NTFS.
pub(crate) fn move_no_replace(from: &str, to: &str) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        // EEXIST, so the message is the OS's own wording ("File exists").
        return Err(with_path(io::Error::from_raw_os_error(17), to));
    }
    fs::rename(from, to).map_err(|e| with_path(e, from))
}

/// Hidden staging name for one item; dot-prefixed so the listing skips it if a crash leaves it
/// behind. Spelled like .NET's `Guid.NewGuid().ToString("D").ToUpperInvariant()` (and Swift's
/// `UUID().uuidString`): 32 random hex digits in the 8-4-4-4-12 groups of a version-4 UUID.
pub fn temp_name() -> String {
    let high = random_u64();
    let low = random_u64();
    // Version 4 in the third group, the RFC 4122 variant in the fourth.
    let high = (high & 0xFFFF_FFFF_FFFF_0FFF) | 0x0000_0000_0000_4000;
    let low = (low & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    format!(
        ".md-reorder-{:08X}-{:04X}-{:04X}-{:04X}-{:012X}",
        high >> 32,
        (high >> 16) & 0xFFFF,
        high & 0xFFFF,
        low >> 48,
        low & 0xFFFF_FFFF_FFFF
    )
}

/// 64 bits no other call in this process shares: std's per-instance randomly keyed SipHash over a
/// process-wide counter, the clock and the process id. md-core takes no `rand`; uniqueness is all a
/// staging name needs, and the counter alone guarantees it within one process.
fn random_u64() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    hasher.write_u32(std::process::id());
    hasher.finish()
}

/// The renames for moving the item at `from` to `to`. Empty when either index is out of range or
/// they are equal. Only names that actually change are listed, in new-order position. The indices
/// are signed so a caller's "one up from the top" (`index - 1` at 0) is simply out of range.
pub fn plan<S: AsRef<str>>(names: &[S], from: isize, to: isize) -> Vec<RenamePair> {
    let count = names.len() as isize;
    if from < 0 || from >= count || to < 0 || to >= count || from == to {
        return Vec::new();
    }
    // Remove first, then insert into the shorter list — Swift's
    // reordered.insert(reordered.remove(at:), at:) — so "to" indexes the final order.
    let mut reordered: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
    let moving = reordered.remove(from as usize);
    reordered.insert(to as usize, moving);
    reordered
        .iter()
        .enumerate()
        .filter_map(|(index, &name)| {
            let renamed = renumbered_name(name, index + 1);
            (renamed != name).then(|| RenamePair::new(name, renamed))
        })
        .collect()
}

/// `name` at 1-based `position`: prefix replaced (or added) as "NN-", stem and extension untouched.
/// Swift's `%02d` — two digits minimum, never a cap: 100 → "100-".
pub fn renumbered_name(name: &str, position: usize) -> String {
    let (base_name, ext) = book_naming::split_extension(name);
    let stem = book_naming::split_prefix(base_name).1;
    if ext.is_empty() { format!("{position:02}-{stem}") } else { format!("{position:02}-{stem}.{ext}") }
}

/// The two-phase steps for `plan` inside `folder`, as full paths, staged under [`temp_name`]s.
pub fn stage(plan: &[RenamePair], folder: &str) -> Vec<StagedRename> {
    stage_with(plan, folder, &mut temp_name)
}

/// Same, with `temp_name` as a seam for deterministic tests.
pub fn stage_with(plan: &[RenamePair], folder: &str, temp_name: &mut dyn FnMut() -> String) -> Vec<StagedRename> {
    plan.iter()
        .map(|pair| StagedRename {
            original: book_paths::combine(folder, &pair.from),
            temp: book_paths::combine(folder, &temp_name()),
            target: book_paths::combine(folder, &pair.to),
        })
        .collect()
}

/// Carry the plan out through `std::fs` (see [`apply_with`]).
pub fn apply(plan: &[RenamePair], folder: &str) -> Result<(), String> {
    apply_with(plan, folder, &LocalBookFileMover)
}

/// Carry the plan out: every item to its temp name, then every temp to its target. A failure at
/// any step moves whatever was staged back to where it came from (best effort — a rollback that
/// fails is not retried) and returns the error text for the "Could not reorder" alert. An empty
/// plan succeeds without touching the disk.
pub fn apply_with(plan: &[RenamePair], folder: &str, mover: &dyn BookFileMover) -> Result<(), String> {
    if plan.is_empty() {
        return Ok(());
    }
    let steps = stage(plan, folder);
    let mut staged: Vec<&StagedRename> = Vec::with_capacity(steps.len());
    let outcome = (|| {
        for step in &steps {
            mover.move_item(&step.original, &step.temp)?;
            staged.push(step);
        }
        for step in &staged {
            mover.move_item(&step.temp, &step.target)?;
        }
        Ok::<(), io::Error>(())
    })();
    match outcome {
        Ok(()) => Ok(()),
        Err(error) => {
            for step in &staged {
                // Nothing better to do — the alert names the first failure.
                let _ = mover.move_item(&step.temp, &step.original);
            }
            Err(error.to_string())
        }
    }
}
