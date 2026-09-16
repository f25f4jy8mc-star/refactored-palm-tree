//! Spaces: one library, and everywhere its contents live.
//!
//! A Space is a place on your disk that you chose. Inside it Archiva keeps
//! the things it makes — the index, the proxies, the notes written here —
//! and a list of the folders you linked into it. Two spaces are two separate
//! libraries: different folders, different tags, different links, and no way
//! for one to see the other.
//!
//! **The database lives inside the space, not in application support.** That
//! is the whole point of the feature, and it is what makes "self-hosted,
//! your data on your disk" true rather than claimed: you can back a space up
//! by copying a folder, move it to another drive, or open it on another
//! machine. A library hidden in an OS-private directory is none of those.
//!
//! Which means this module cannot use a database to find its own data — the
//! registry of spaces is a small JSON file beside the app's settings, read
//! before anything is open. Bootstrapping a database in order to discover
//! where the databases are is circular.
//!
//! Two records, deliberately:
//!
//!   * the **registry** (`spaces.json`, in app data) lists the spaces this
//!     machine knows about and which one was last open;
//!   * a **stamp** (`archiva-space.json`, in the space) carries the space's
//!     own id and name, so a folder you moved by hand, copied to a backup
//!     drive or were sent still says what it is. The registry is this
//!     machine's memory; the stamp is the space's identity, and identity
//!     travels with the thing (rule 2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::scan::uuid_v7;

/// What a space is called before it is called anything.
pub const DEFAULT_NAME: &str = "My Archive";

/// The index, inside the space.
pub const DB_FILE: &str = "archiva.sqlite";
/// The space's own identity, so a folder moved by hand still says what it is.
pub const STAMP_FILE: &str = "archiva-space.json";
/// Where generated artefacts go. Made on creation so a space is recognisable
/// as one before anything has been indexed into it.
pub const MADE_HERE_DIRS: &[&str] = &["proxies", "notes"];

const REGISTRY_FILE: &str = "spaces.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: String,
    pub name: String,
    /// Where it lives now. Absolute.
    pub root: String,
    pub created_at: String,
    pub last_opened_at: Option<String>,
}

impl Space {
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.root)
    }
    pub fn db_path(&self) -> PathBuf {
        self.path().join(DB_FILE)
    }
    /// Whether the folder is still there with a space in it. A drive that is
    /// not plugged in is the ordinary case, not an error.
    pub fn reachable(&self) -> bool {
        self.path().join(STAMP_FILE).is_file()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    spaces: Vec<Space>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stamp {
    id: String,
    name: String,
}

/* ------------------------------------------------------------- registry */

fn registry_path(app_data: &Path) -> PathBuf {
    app_data.join(REGISTRY_FILE)
}

fn read_registry(app_data: &Path) -> Result<Registry> {
    let path = registry_path(app_data);
    if !path.exists() {
        return Ok(Registry::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    // A registry that will not parse is a worse thing to lose than to
    // rebuild: a space is still openable by its folder, and refusing to
    // start over a malformed settings file would strand the user entirely.
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

fn write_registry(app_data: &Path, reg: &Registry) -> Result<()> {
    std::fs::create_dir_all(app_data)?;
    let path = registry_path(app_data);
    // Write beside and rename, so an interrupted write cannot leave the list
    // of every library this machine knows about half-written.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(reg)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Every space this machine knows about, most recently opened first.
pub fn list(app_data: &Path) -> Result<Vec<Space>> {
    let mut spaces = read_registry(app_data)?.spaces;
    spaces.sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at).then(a.name.cmp(&b.name)));
    Ok(spaces)
}

/// The space that was open when the app last closed, if it is still there.
pub fn current(app_data: &Path) -> Result<Option<Space>> {
    let reg = read_registry(app_data)?;
    let Some(id) = reg.current else { return Ok(None) };
    Ok(reg.spaces.into_iter().find(|s| s.id == id))
}

fn get(app_data: &Path, id: &str) -> Result<Space> {
    read_registry(app_data)?
        .spaces
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| anyhow!("no space with that id is known here"))
}

fn save(app_data: &Path, space: Space) -> Result<()> {
    let mut reg = read_registry(app_data)?;
    reg.version = 1;
    match reg.spaces.iter_mut().find(|s| s.id == space.id) {
        Some(slot) => *slot = space,
        None => reg.spaces.push(space),
    }
    write_registry(app_data, &reg)
}

/* -------------------------------------------------------------- making */

/// Make a space at `root`, and remember it.
///
/// The folder may exist — pointing at an empty folder you made yourself is
/// the ordinary way to do this — but it must not already hold a *different*
/// space, and it must not hold other files: a space owns its folder, and
/// writing an index into someone's Documents root would be a surprise nobody
/// asked for. An existing space at that path is adopted rather than refused,
/// which is what "open the one on my backup drive" needs.
pub fn create(app_data: &Path, name: &str, root: &Path) -> Result<Space> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("a space needs a name"));
    }
    if root.as_os_str().is_empty() {
        return Err(anyhow!("a space needs somewhere to live"));
    }

    if let Some(existing) = stamped(app_data, root)? {
        return Ok(existing);
    }
    if root.exists() && !is_empty_dir(root)? {
        return Err(anyhow!(
            "{} already has things in it. A space keeps its own index, \
             proxies and notes here, so it wants a folder of its own.",
            root.display()
        ));
    }

    std::fs::create_dir_all(root).with_context(|| format!("making {}", root.display()))?;
    for dir in MADE_HERE_DIRS {
        std::fs::create_dir_all(root.join(dir))?;
    }

    let space = Space {
        id: uuid_v7(),
        name: name.to_string(),
        root: root.to_string_lossy().to_string(),
        created_at: super::signals::fmt_time(std::time::SystemTime::now()),
        last_opened_at: None,
    };
    write_stamp(root, &space)?;
    // Make the index now rather than on first use: a space that exists but
    // has no database is a state every later call would have to handle.
    drop(crate::db::open(&space.db_path())?);
    save(app_data, space.clone())?;
    Ok(space)
}

/// The space already living at `root`, adopted into this machine's registry.
/// `None` when there is nothing there.
fn stamped(app_data: &Path, root: &Path) -> Result<Option<Space>> {
    let stamp_at = root.join(STAMP_FILE);
    if !stamp_at.is_file() {
        return Ok(None);
    }
    let stamp: Stamp = serde_json::from_str(&std::fs::read_to_string(&stamp_at)?)
        .with_context(|| format!("reading {}", stamp_at.display()))?;

    let mut reg = read_registry(app_data)?;
    // Known already, but perhaps at an old path — a folder the user moved
    // themselves. The stamp is the identity; the path is just where it is
    // today.
    if let Some(known) = reg.spaces.iter_mut().find(|s| s.id == stamp.id) {
        known.root = root.to_string_lossy().to_string();
        known.name = stamp.name;
        let found = known.clone();
        write_registry(app_data, &reg)?;
        return Ok(Some(found));
    }

    let space = Space {
        id: stamp.id,
        name: stamp.name,
        root: root.to_string_lossy().to_string(),
        created_at: super::signals::fmt_time(std::time::SystemTime::now()),
        last_opened_at: None,
    };
    save(app_data, space.clone())?;
    Ok(Some(space))
}

/// Open a folder as a space: adopt the one already there, or make one.
pub fn open_folder(app_data: &Path, root: &Path) -> Result<Space> {
    match stamped(app_data, root)? {
        Some(space) => Ok(space),
        None => {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| DEFAULT_NAME.to_string());
            create(app_data, &name, root)
        }
    }
}

fn write_stamp(root: &Path, space: &Space) -> Result<()> {
    let stamp = Stamp {
        id: space.id.clone(),
        name: space.name.clone(),
    };
    std::fs::write(root.join(STAMP_FILE), serde_json::to_string_pretty(&stamp)?)?;
    Ok(())
}

fn is_empty_dir(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Err(anyhow!("{} is not a folder", path.display()));
    }
    Ok(std::fs::read_dir(path)?.next().is_none())
}

/* ------------------------------------------------------------- opening */

/// Open a space's index, and record that it is the current one.
pub fn open(app_data: &Path, id: &str) -> Result<(Space, Connection)> {
    let mut space = get(app_data, id)?;
    if !space.reachable() {
        return Err(anyhow!(
            "{} isn’t there. The folder may have been moved or its drive isn’t connected — \
             nothing has been lost.",
            space.root
        ));
    }
    let conn = crate::db::open(&space.db_path())?;
    space.last_opened_at = Some(super::signals::fmt_time(std::time::SystemTime::now()));
    save(app_data, space.clone())?;

    let mut reg = read_registry(app_data)?;
    reg.current = Some(space.id.clone());
    write_registry(app_data, &reg)?;
    Ok((space, conn))
}

/* ------------------------------------------------------------ changing */

pub fn rename(app_data: &Path, id: &str, name: &str) -> Result<Space> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("a space needs a name"));
    }
    let mut space = get(app_data, id)?;
    space.name = name.to_string();
    // The stamp travels with the folder, so the new name has to go in both
    // or a space moved to another machine would arrive under its old one.
    if space.reachable() {
        write_stamp(&space.path(), &space)?;
    }
    save(app_data, space.clone())?;
    Ok(space)
}

/// Move a space's folder, with everything in it.
///
/// The caller closes the index first — a database cannot be moved out from
/// under an open connection, and on Windows it cannot be moved at all.
pub fn move_to(app_data: &Path, id: &str, new_root: &Path) -> Result<Space> {
    let mut space = get(app_data, id)?;
    let from = space.path();
    if from == new_root {
        return Ok(space);
    }
    if !space.reachable() {
        return Err(anyhow!("{} isn’t there to move", space.root));
    }
    if new_root.exists() && !is_empty_dir(new_root)? {
        return Err(anyhow!("{} already has things in it", new_root.display()));
    }
    if new_root.starts_with(&from) {
        return Err(anyhow!("a space cannot be moved inside itself"));
    }

    if let Some(parent) = new_root.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A rename is atomic and instant on the same volume. Across volumes it
    // fails, and the honest fallback is a copy — the space is the user's
    // whole library and half of it in each place would be worse than either.
    if std::fs::rename(&from, new_root).is_err() {
        copy_tree(&from, new_root)
            .with_context(|| format!("copying {} to {}", from.display(), new_root.display()))?;
        std::fs::remove_dir_all(&from)
            .with_context(|| format!("removing {} after the copy", from.display()))?;
    }

    space.root = new_root.to_string_lossy().to_string();
    write_stamp(new_root, &space)?;
    save(app_data, space.clone())?;
    Ok(space)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Stop listing a space on this machine. **Never** deletes the folder: the
/// space is the user's data, and forgetting where it is has to be recoverable
/// by pointing at it again.
pub fn forget(app_data: &Path, id: &str) -> Result<()> {
    let mut reg = read_registry(app_data)?;
    reg.spaces.retain(|s| s.id != id);
    if reg.current.as_deref() == Some(id) {
        reg.current = None;
    }
    write_registry(app_data, &reg)
}

/// Where a space keeps one kind of thing it made. Made if it is missing, so
/// a space copied without its empty folders still works.
pub fn made_here(space: &Space, what: &str) -> Result<PathBuf> {
    let dir = space.path().join(what);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A space plus the three things only the machine in front of you can say:
/// whether its folder is there right now, whether it is the one open, and how
/// much of it Archiva wrote.
///
/// In the model rather than in `commands` so there is one definition of what
/// a space looks like to a view — and so the walkthrough fixture can be
/// emitted from the real thing rather than from a shape written by hand.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Described {
    #[serde(flatten)]
    pub space: Space,
    pub reachable: bool,
    pub is_current: bool,
    /// Bytes, by what made them — `index`, `proxies`, `notes`.
    pub holds: BTreeMap<String, u64>,
}

pub fn describe(space: Space, current: Option<&str>) -> Described {
    let reachable = space.reachable();
    let holds = if reachable { sizes(&space) } else { BTreeMap::new() };
    Described {
        is_current: current == Some(space.id.as_str()),
        reachable,
        holds,
        space,
    }
}

/// What a space holds, for showing beside its name. Counted from disk rather
/// than stored: a number that can go stale is a number that will.
pub fn sizes(space: &Space) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for what in MADE_HERE_DIRS {
        out.insert(what.to_string(), dir_bytes(&space.path().join(what)));
    }
    out.insert("index".into(), file_bytes(&space.db_path()));
    out
}

fn file_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn dir_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            _ => file_bytes(&e.path()),
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archiva-spaces-{}", uuid_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_new_space_is_a_folder_you_chose_with_an_index_in_it() {
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("Archive");

        let space = create(&app, "My Archive", &root).unwrap();
        assert_eq!(space.name, "My Archive");
        assert!(root.join(DB_FILE).is_file(), "the index lives in the space");
        assert!(root.join(STAMP_FILE).is_file());
        assert!(root.join("proxies").is_dir());
        assert!(root.join("notes").is_dir());
        assert!(space.reachable());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn the_registry_is_not_inside_any_space() {
        // It has to be readable before anything is open, and it has to
        // survive a space being moved to another drive.
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("Archive");
        create(&app, "One", &root).unwrap();
        assert!(app.join(REGISTRY_FILE).is_file());
        assert!(!root.join(REGISTRY_FILE).exists());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn two_spaces_are_two_libraries() {
        let home = scratch();
        let app = home.join("app-data");
        let a = create(&app, "Work", &home.join("work")).unwrap();
        let b = create(&app, "Home", &home.join("home")).unwrap();
        assert_ne!(a.id, b.id);
        assert_ne!(a.db_path(), b.db_path());
        assert_eq!(list(&app).unwrap().len(), 2);
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_folder_with_other_things_in_it_is_refused() {
        // A space owns its folder. Writing an index into someone's Documents
        // root is a surprise nobody asked for.
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("not-empty");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("holiday.jpg"), b"x").unwrap();

        let err = create(&app, "Nope", &root).unwrap_err().to_string();
        assert!(err.contains("already has things in it"), "{err}");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn pointing_at_a_space_that_already_exists_adopts_it() {
        // The backup-drive case, and the second-machine case. The stamp is
        // the identity, so it comes back as the same space rather than a
        // second one pointing at the same files.
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("Archive");
        let made = create(&app, "My Archive", &root).unwrap();

        let other = home.join("other-machine");
        let adopted = open_folder(&other, &root).unwrap();
        assert_eq!(adopted.id, made.id, "same space, not a copy of one");
        assert_eq!(adopted.name, "My Archive");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_space_moved_by_hand_is_found_again_by_its_stamp() {
        let home = scratch();
        let app = home.join("app-data");
        let was = home.join("Archive");
        let made = create(&app, "My Archive", &was).unwrap();

        let now = home.join("Elsewhere");
        std::fs::rename(&was, &now).unwrap();
        let found = open_folder(&app, &now).unwrap();
        assert_eq!(found.id, made.id);
        assert_eq!(found.root, now.to_string_lossy());
        assert_eq!(list(&app).unwrap().len(), 1, "moved, not duplicated");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn opening_records_which_one_is_current() {
        let home = scratch();
        let app = home.join("app-data");
        let a = create(&app, "Work", &home.join("work")).unwrap();
        let b = create(&app, "Home", &home.join("home")).unwrap();

        assert!(current(&app).unwrap().is_none(), "nothing is open yet");
        let (opened, conn) = open(&app, &b.id).unwrap();
        drop(conn);
        assert_eq!(opened.id, b.id);
        assert_eq!(current(&app).unwrap().unwrap().id, b.id);
        assert_ne!(current(&app).unwrap().unwrap().id, a.id);
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_space_on_a_drive_that_is_not_there_says_so_rather_than_failing_oddly() {
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("Archive");
        let space = create(&app, "My Archive", &root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        assert!(!space.reachable());
        let err = open(&app, &space.id).unwrap_err().to_string();
        assert!(err.contains("nothing has been lost"), "{err}");
        assert_eq!(list(&app).unwrap().len(), 1, "still listed, still findable");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn renaming_writes_the_new_name_where_the_space_can_carry_it() {
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("Archive");
        let space = create(&app, "My Archive", &root).unwrap();

        rename(&app, &space.id, "Studio").unwrap();
        assert_eq!(list(&app).unwrap()[0].name, "Studio");

        // And on another machine, which only has the folder.
        let elsewhere = home.join("other-machine");
        assert_eq!(open_folder(&elsewhere, &root).unwrap().name, "Studio");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn moving_takes_everything_with_it() {
        let home = scratch();
        let app = home.join("app-data");
        let was = home.join("Archive");
        let space = create(&app, "My Archive", &was).unwrap();
        std::fs::write(was.join("proxies/thumb.jpg"), b"proxy").unwrap();

        let now = home.join("drive/Archive");
        let moved = move_to(&app, &space.id, &now).unwrap();
        assert_eq!(moved.root, now.to_string_lossy());
        assert!(now.join(DB_FILE).is_file());
        assert_eq!(std::fs::read(now.join("proxies/thumb.jpg")).unwrap(), b"proxy");
        assert!(!was.exists(), "and leaves nothing behind");
        assert_eq!(list(&app).unwrap()[0].root, now.to_string_lossy());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_move_onto_something_else_is_refused_rather_than_merged() {
        let home = scratch();
        let app = home.join("app-data");
        let space = create(&app, "My Archive", &home.join("Archive")).unwrap();
        let taken = home.join("taken");
        std::fs::create_dir_all(&taken).unwrap();
        std::fs::write(taken.join("something.txt"), b"x").unwrap();

        assert!(move_to(&app, &space.id, &taken).is_err());
        assert!(space.path().join(DB_FILE).is_file(), "and changes nothing");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_space_cannot_be_moved_inside_itself() {
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("Archive");
        let space = create(&app, "My Archive", &root).unwrap();
        assert!(move_to(&app, &space.id, &root.join("inner")).is_err());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn forgetting_a_space_never_deletes_it() {
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("Archive");
        let space = create(&app, "My Archive", &root).unwrap();
        open(&app, &space.id).unwrap();

        forget(&app, &space.id).unwrap();
        assert!(list(&app).unwrap().is_empty());
        assert!(current(&app).unwrap().is_none(), "and stops being the current one");
        assert!(root.join(DB_FILE).is_file(), "the library is still on disk");

        // And can be picked up again by pointing at the folder.
        assert_eq!(open_folder(&app, &root).unwrap().id, space.id);
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_registry_that_will_not_parse_does_not_strand_the_user() {
        let home = scratch();
        let app = home.join("app-data");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join(REGISTRY_FILE), "{ not json").unwrap();

        assert!(list(&app).unwrap().is_empty());
        let space = create(&app, "Fresh", &home.join("Archive")).unwrap();
        assert_eq!(list(&app).unwrap()[0].id, space.id);
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn what_a_space_holds_is_counted_from_disk() {
        let home = scratch();
        let app = home.join("app-data");
        let root = home.join("Archive");
        let space = create(&app, "My Archive", &root).unwrap();
        std::fs::write(root.join("proxies/a.jpg"), vec![0u8; 512]).unwrap();

        let sizes = sizes(&space);
        assert_eq!(sizes["proxies"], 512);
        assert!(sizes["index"] > 0, "the index is a real file");
        std::fs::remove_dir_all(&home).ok();
    }
}
