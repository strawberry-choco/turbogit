//! Git stash operations and the IDE "shelf" (patch store).
//!
//! The stash functions are thin pass-throughs to [`GitExecutor`]. The shelf is a
//! pure-Rust IDE patch store: named buckets of affected file paths persisted by
//! the caller as RON under `.turbogit/shelf.ron`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

// ---------- Stash pass-throughs ----------

/// Create a stash entry for `root`.
pub fn stash(vcs: &dyn GitExecutor, root: &Path, message: &str, keep_index: bool) -> TgResult<()> {
    vcs.stash_push(root, message, keep_index)
}

/// List all stash entries for `root`.
pub fn list(vcs: &dyn GitExecutor, root: &Path) -> TgResult<Vec<Stash>> {
    vcs.stash_list(root)
}

// ---------- IDE shelf (patch store) ----------

/// Build a new [`Shelf`] capturing `changes` under `name`, owned by `root`.
pub fn make_shelf(name: &str, changes: &[Change], root: RootId) -> Shelf {
    Shelf {
        name: name.to_string(),
        changes: changes.to_vec(),
        created_at: chrono::Utc::now(),
        root,
        source: ShelfSource::Stash,
    }
}

/// Build the [`Shelf`] record for an **imported patch**: the same named bucket, whose
/// content is the patch text in [`patch_payload_path`] rather than a stash entry.
///
/// `file` is the payload this record was written beside, so the row and its bytes can
/// never disagree about which file holds them — and so an import that the index refused
/// can delete exactly the file it made.
pub fn make_patch_shelf(
    name: &str,
    changes: &[Change],
    root: RootId,
    created_at: chrono::DateTime<chrono::Utc>,
    file: &str,
) -> Shelf {
    Shelf {
        name: name.to_string(),
        changes: changes.to_vec(),
        created_at,
        root,
        source: ShelfSource::Patch {
            file: file.to_string(),
        },
    }
}

// ---------- Imported patch payloads ----------

/// The directory holding an imported shelf's patch bytes, beside the index that names
/// them.
///
/// The content lives in a **file per shelf** rather than inside `shelf.ron` because the
/// index is rewritten whole on every verb: a patch's bytes in that file would make every
/// shelve a re-serialisation of every imported patch this project ever parked, and a
/// refused rewrite would put them all at risk for nothing. This is also the shape the
/// reference's own shelf uses — a directory of `.patch` files addressed by the record.
pub fn patch_payload_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".turbogit").join("shelf")
}

/// Where one imported shelf's bytes live.
pub fn patch_payload_path(project_dir: &Path, file: &str) -> PathBuf {
    patch_payload_dir(project_dir).join(file)
}

/// The payload file **stem** for a shelf: a slug of its name plus its timestamp.
///
/// The timestamp is load-bearing, not tidiness — two shelves may share a name (the
/// pane lists both), and a payload file keyed on the name alone would have the second
/// import overwrite the first one's parked bytes while both rows still pointed at it.
/// It is a stem rather than a file name because [`write_patch_payload`] adds the counter
/// that makes a same-nanosecond pair distinct, and the suffix has to come after it.
pub fn patch_payload_stem(name: &str, created_at: chrono::DateTime<chrono::Utc>) -> String {
    let mut slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-').chars().take(40).collect::<String>();
    let slug = if slug.is_empty() { "patch" } else { &slug };
    format!(
        "{slug}-{}",
        created_at.timestamp_nanos_opt().unwrap_or_default()
    )
}

/// Write an imported patch's bytes to the payload store and return the file name they
/// went under.
///
/// **Payload first, index second** is the ordering this function exists to make
/// unavoidable. The alternative — record, then bytes — leaves a row naming a file that
/// was never made if the write is refused by a full disk or a read-only directory, and
/// `Unshelve` on that row can only fail with a message about a missing file. Reversed,
/// the residue is an unreferenced `.patch` nobody lists, which [`delete_patch_payload`]
/// removes on the refusal path anyway. A stray file is recoverable; a row pointing at
/// nothing is the stranded-work bug wearing an imported name.
///
/// The name is also **disambiguated against the directory**, because a timestamp alone is
/// not unique: two imports in one second land on the same `created_at` on a fast machine,
/// and two rows whose payload is the same file is one row's bytes silently replacing the
/// other's — a deferral that destroys what it deferred. So a taken name gains a counter,
/// and every row owns exactly one file.
pub fn write_patch_payload(
    project_dir: &Path,
    name: &str,
    created_at: chrono::DateTime<chrono::Utc>,
    text: &str,
) -> TgResult<String> {
    let stem = patch_payload_stem(name, created_at);
    let dir = patch_payload_dir(project_dir);
    fs::create_dir_all(&dir)?;
    let mut taken = 1usize;
    let file = loop {
        let candidate = if taken == 1 {
            format!("{stem}.patch")
        } else {
            format!("{stem}-{taken}.patch")
        };
        if !dir.join(&candidate).exists() {
            break candidate;
        }
        taken += 1;
    };
    fs::write(dir.join(&file), text)?;
    Ok(file)
}

/// Read an imported shelf's parked bytes back. A payload that is not there is an error
/// with a name in it, because the row still lists the files the patch carried and the
/// developer deserves to know which half went missing.
pub fn read_patch_payload(project_dir: &Path, file: &str) -> TgResult<String> {
    let path = patch_payload_path(project_dir, file);
    fs::read_to_string(&path).map_err(|e| {
        turbogit_domain::error::TgError::Other(format!(
            "The patch this shelf was imported from is not at {} — {e}",
            path.display()
        ))
    })
}

/// Delete an imported shelf's payload: the cleanup that follows a row leaving the shelf,
/// and the rollback of a refused index write.
///
/// A file that is already gone is **not a failure** — the verb is reclaiming space, and
/// nothing is left to reclaim.
pub fn delete_patch_payload(project_dir: &Path, file: &str) -> TgResult<()> {
    let path = patch_payload_path(project_dir, file);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// The shelf name a stash entry's message carries, or `None` when the entry was
/// not written by a shelve.
///
/// **The shelf's content IS a git stash entry**: [`crate::shelve_stash::stash`]
/// parks the work with `git stash push -m <shelf name>`, and git stores a named
/// stash as `On <branch>: <name>` (an unnamed one reads `WIP on <branch>:
/// <sha> <subject>`). Unshelving therefore has to find the entry again by name,
/// and the two adapters hand back the message at different depths — the CLI
/// strips `stash@{n}: ` when it parses the listing, libgit2 never had it — so
/// this recognises both the prefixed and the bare spelling rather than betting
/// on one. A name that contains `": "` survives the split because only the
/// leading `On <branch>` prefix is removed.
pub fn shelf_name_of(message: &str) -> Option<&str> {
    let rest = message
        .strip_prefix("WIP on ")
        .or_else(|| message.strip_prefix("On "))
        .unwrap_or(message);
    match rest.split_once(": ") {
        // `On <branch>` with no message after it: not a shelve's entry.
        Some((_, "")) => None,
        Some((_, name)) => Some(name),
        None => (!rest.is_empty()).then_some(rest),
    }
}

/// The stash entry holding `name`'s shelved content — the newest match, since
/// `git stash list` is newest-first and two shelves may share a name.
pub fn find_shelf_stash<'a>(entries: &'a [Stash], name: &str) -> Option<&'a Stash> {
    entries
        .iter()
        .find(|s| shelf_name_of(&s.message) == Some(name))
}

/// Put a shelf's content back in the working tree.
///
/// `remove` is the difference between the reference's two verbs with **opposite
/// keep/remove defaults**: `false` is `Unshelve` (the shelved copy stays where
/// it is), `true` is `Unshelve and Remove` (the git copy goes with it, which is
/// what makes the shelf row's own removal safe). Returns the stash entry it
/// applied, so the caller can name it in what it reports.
pub fn unshelve(vcs: &dyn GitExecutor, root: &Path, name: &str, remove: bool) -> TgResult<Stash> {
    let entries = list(vcs, root)?;
    let found = find_shelf_stash(&entries, name).cloned().ok_or_else(|| {
        turbogit_domain::error::TgError::Other(format!(
            "Nothing shelved as '{name}' is left in this repository's stash list"
        ))
    })?;
    if remove {
        vcs.stash_pop(root, found.index)?;
    } else {
        vcs.stash_apply(root, found.index)?;
    }
    Ok(found)
}

/// Throw away the stash entry a shelf's content was parked in, when it is still
/// there. A shelf whose entry is already gone has nothing left on the git side,
/// which is a normal answer rather than a failure: the verb is deleting the
/// record, and this is the cleanup that follows it.
pub fn discard_shelf_content(vcs: &dyn GitExecutor, root: &Path, name: &str) -> TgResult<()> {
    if let Some(found) = find_shelf_stash(&list(vcs, root)?, name) {
        let index = found.index;
        vcs.stash_drop(root, index)?;
    }
    Ok(())
}

/// How one stash entry is named to a person: git's own ref spelling.
pub fn stash_ref(index: usize) -> String {
    format!("stash@{{{index}}}")
}

/// Path to the persisted shelf store for `project_dir`.
pub fn shelf_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".turbogit").join("shelf.ron")
}

/// The wire's **absent** marker for [`StoredShelf::root`]: a row written before
/// ticket 12 has no `root` key at all, and serde needs some value for it.
///
/// An empty path is used rather than `Option<RootId>` because RON 0.12 reads an
/// `Option` only in its `Some(..)` spelling, while this build writes `Shelf`'s
/// root as a plain value — so an `Option` here fails to read back the very file
/// this build wrote. The marker never escapes the wire: [`StoredShelf::into_shelf`]
/// replaces it on the way in, and [`save_shelves`] re-creates it on the way out
/// purely to drop the key, so a [`Shelf`] always carries a real root.
fn absent_root() -> RootId {
    RootId(Arc::from(Path::new("")))
}

/// Whether a root is [`absent_root`], the wire's **absent** spelling.
///
/// One predicate serves both directions on purpose: `skip_serializing_if` drops the
/// `root` key exactly where `#[serde(default)]` would supply it, so "no root on the
/// wire" and "the marker in memory" cannot drift into two different meanings.
fn is_absent_root(root: &RootId) -> bool {
    root.as_path().as_os_str().is_empty()
}

/// The store's **on-disk row shape**: a [`Shelf`] whose `root` may be absent.
///
/// It is a mirror rather than a flag day, because the two shapes are genuinely
/// different facts. [`Shelf::root`] is required — an unshelve parks or restores
/// work in exactly one repository, so a rootless shelf is not a thing the app can
/// act on — while a row written by a build before ticket 12 simply predates the
/// field. Reading the file into `Shelf` directly would make every legacy store
/// unparseable *as a whole*, which is how one old row used to take the launch
/// load down with it.
///
/// Constructing the `Shelf` here by name, with no `..` rest pattern, is the drift
/// guard: when `Shelf` grows a field this mirror does not carry, the store fails
/// to compile rather than silently reading it back as a default.
#[derive(serde::Deserialize)]
struct StoredShelf {
    name: String,
    changes: Vec<Change>,
    created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default = "absent_root")]
    root: RootId,
    /// Absent on every row written before an imported patch could be parked, and an
    /// absent one **is** a stash row: that was the only mechanism there was. The default
    /// therefore lands on the truth rather than on a guess, which is what keeps
    /// [`unshelve`] going to git's stash list for a row it invented nothing about.
    #[serde(default)]
    source: ShelfSource,
}

/// The same row on its way **out**, borrowed so a save copies nothing it does not
/// have to.
///
/// Serialising [`Shelf`] itself would write a `root` for every row, including the
/// one a legacy load invented — see [`save_shelves`], which fills this mirror's
/// `root` with [`absent_root`] for such a row so the key is dropped and the store
/// keeps the shape it was given.
#[derive(serde::Serialize)]
struct StoredShelfOut<'a> {
    name: &'a str,
    changes: &'a [Change],
    created_at: &'a chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "is_absent_root")]
    root: RootId,
    /// A stash row goes back with **no `source` key**, exactly as it arrived, so a store
    /// this build rewrites is byte-comparable with one it merely read. Only an imported
    /// row gains the key — and it must, because the file its bytes live in is a fact
    /// nothing else records.
    #[serde(skip_serializing_if = "ShelfSource::is_stash")]
    source: &'a ShelfSource,
}

impl StoredShelf {
    /// Bind a row to its root: the one it recorded, or — for a row written before
    /// `root` existed — `project_dir`, the directory that holds this very store.
    ///
    /// That is the only root a legacy row could have meant: the store lives at
    /// `<project_dir>/.turbogit/shelf.ron`, and [`crate::multi_root::scan_for_roots`]
    /// registers `project_dir` itself as a root whenever it is a repository, which
    /// is the single-root case every legacy store was written by. Binding it there
    /// keeps the row listable and unshelve-able instead of dropping it.
    ///
    /// The cost is honest and bounded: in a **multi-root** project whose
    /// `project_dir` is not itself a repository, a legacy row lands on a root no
    /// pane lists, so it reads as missing rather than as applied to the wrong
    /// tree — and it is never destroyed, both because [`save_shelves`] will not
    /// truncate a store over rows it has not read and because the binding it
    /// writes back is the absence it read, never this path.
    fn into_shelf(self, project_dir: &Path) -> Shelf {
        let root = if is_absent_root(&self.root) {
            RootId(Arc::from(project_dir))
        } else {
            self.root
        };
        Shelf {
            name: self.name,
            changes: self.changes,
            created_at: self.created_at,
            root,
            source: self.source,
        }
    }
}

/// A file with nothing in it is an **empty shelf**, not an unreadable one: it
/// carries no records for a write to destroy. Without this, a 0-byte
/// `shelf.ron` — what an interrupted `fs::write` leaves behind — would refuse
/// every later shelve on a store that in fact holds nothing.
fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// Read the shelf store back. A file that is not there is an empty shelf — the
/// store is created by the first shelve, not at launch.
///
/// A row written before ticket 12 added [`Shelf::root`] is read, not dropped: see
/// [`StoredShelf`].
///
/// A file this build cannot parse at all is an **error, not an empty list**: those
/// rows are the user's shelved work, and a pane that answered "nothing is shelved"
/// over a store it failed to read would be reporting an absence it has not
/// established. Such a file is also un-writable — see [`save_shelves`].
pub fn load_shelves(project_dir: &Path) -> TgResult<Vec<Shelf>> {
    let path = shelf_path(project_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    if is_blank(&text) {
        return Ok(Vec::new());
    }
    let rows: Vec<StoredShelf> =
        ron::from_str(&text).map_err(|e| turbogit_domain::error::TgError::Serde(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|row| row.into_shelf(project_dir))
        .collect())
}

/// Serialize `shelves` to `shelf.ron` under `.turbogit/`.
///
/// **A write is a truncation**, so it first establishes that the file it is about
/// to erase holds nothing it has not read. This is the store's own invariant, not
/// caller discipline: both app-side writers pass the in-memory slice and neither
/// can be trusted to remember the guard — before it existed, a launch load that
/// failed left that slice empty and the next shelve or shelf removal wrote it over
/// the user's whole shelf index, stranding work whose name was the only way
/// [`unshelve`] could find it again.
///
/// So: when a store exists and this build cannot read it — unparsable, or
/// unreadable at all — this refuses with an error and the file is left
/// byte-for-byte alone. The caller reports it; nothing is lost.
///
/// A row whose root **is** `project_dir` goes back the way an old row arrived: with
/// no `root` key. The two spellings are one fact to this store — [`StoredShelf`]
/// reads an absent root as `project_dir`, so writing the path adds nothing — and
/// recording it would freeze into the index a location that need not be a
/// repository, which the next load would hand back as a `RootId` matching nothing.
/// The read-time binding of a legacy row therefore stays read-time.
///
/// One deliberate limit: the guard is **check-then-write** — parsed at t0, truncated
/// at t1, with no lock and no atomic rename — so it defends this session's own rows,
/// not a second process writing the same file.
pub fn save_shelves(project_dir: &Path, shelves: &[Shelf]) -> TgResult<()> {
    let path = shelf_path(project_dir);
    let unreadable = path.exists()
        && match fs::read_to_string(&path) {
            // Cannot establish what the file holds, so assume it holds records.
            Err(_) => true,
            Ok(text) => !is_blank(&text) && ron::from_str::<Vec<StoredShelf>>(&text).is_err(),
        };
    if unreadable {
        return Err(turbogit_domain::error::TgError::Other(format!(
            "Refused to rewrite the shelf store at {}, which this build cannot read; \
             its records would be lost. Repair or remove that file to shelve again.",
            path.display()
        )));
    }
    let rows: Vec<StoredShelfOut<'_>> = shelves
        .iter()
        .map(|shelf| StoredShelfOut {
            name: &shelf.name,
            changes: &shelf.changes,
            created_at: &shelf.created_at,
            root: if shelf.root.as_path() == project_dir {
                absent_root()
            } else {
                shelf.root.clone()
            },
            source: &shelf.source,
        })
        .collect();
    fs::create_dir_all(project_dir.join(".turbogit"))?;
    let text = ron::ser::to_string_pretty(&rows, ron::ser::PrettyConfig::new())
        .map_err(|e| turbogit_domain::error::TgError::Serde(e.to_string()))?;
    fs::write(path, text)?;
    Ok(())
}
