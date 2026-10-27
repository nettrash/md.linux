//! `KeyFileSettingsStore` — md-logic's [`SettingsStore`] over a `glib::KeyFile` at
//! `$XDG_CONFIG_HOME/md/settings.conf`, one group `[md]` (shell-design §9.1).
//!
//! Why GKeyFile: it is in GLib already (no new dependency for a dozen keys), it escapes what a
//! value needs, `save_to_file` is `g_file_set_contents` — a temporary file renamed over the old
//! one, so a crash never leaves half a settings file — and `key=value` is what Linux users and
//! PRIVACY.md readers recognise.
//!
//! Semantics are md-logic's `InMemorySettingsStore`'s: values are strings or bools, a string read
//! of a bool (or the reverse) is absent, and [`SettingsStore::changed`] fires only when the stored
//! value differs — after the file was written, with no borrow held. Every `set_*` / `remove`
//! writes the file through (settings change at human speed).
//!
//! On disk the type of a value is the type of its key: the four [`BOOL_KEYS`] are written with
//! `set_boolean` and read back with `boolean`, every other key as a string. A value of the other
//! type behaves exactly as in memory for the life of the process; after a restart a string stored
//! under a bool key reads as a bool if it spells one (`true`, `false`, `1`, `0`), and a bool stored
//! under a string key reads as the string `true` / `false`. The app never writes either.
//!
//! Failure modes: a file that does not parse is renamed `settings.conf.corrupt` and the store
//! starts empty; a file that cannot be read (permissions, a folder in its place) is left alone and
//! the store runs in memory; a folder that cannot be written keeps the store in memory with one
//! warning on stderr.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use md_logic::event::Event;
use md_logic::settings::settings_keys;
use md_logic::settings::{SettingValue, SettingsStore};

/// The keys stored as booleans; every other key is a string (shell-design §9.1).
pub const BOOL_KEYS: [&str; 4] = [
    settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS,
    settings_keys::CONTINUE_LISTS,
    settings_keys::CAPITALIZE_SENTENCES,
    settings_keys::WRAP_SELECTION,
];

/// The one group of `settings.conf`.
const GROUP: &str = "md";
/// Appended to the name of a settings file that did not parse.
const CORRUPT_SUFFIX: &str = ".corrupt";

/// md's settings, persisted in `settings.conf`. One per process ([`Shell::settings`]), shared by
/// every window.
///
/// [`Shell::settings`]: crate::app::Shell::settings
pub struct KeyFileSettingsStore {
    path: PathBuf,
    /// In the order each key was first stored (the file's order after a load).
    values: RefCell<Vec<(String, SettingValue)>>,
    /// False when the file exists but could not be read: never overwrite what we could not see.
    persist: bool,
    /// The one stderr warning for an unwritable folder has been given.
    warned: Cell<bool>,
    changed: Event<str>,
}

impl KeyFileSettingsStore {
    /// Loads `path` (missing → empty; corrupt → renamed aside, empty; unreadable → empty, in
    /// memory only).
    pub fn open(path: PathBuf) -> KeyFileSettingsStore {
        let (values, persist) = match load(&path) {
            Load::Values(values) => (values, true),
            Load::Missing => (Vec::new(), true),
            Load::Corrupt(message) => {
                let aside = corrupt_path(&path);
                match std::fs::rename(&path, &aside) {
                    Ok(()) => eprintln!(
                        "md: {} could not be read ({message}); kept as {} and starting with no settings",
                        path.display(),
                        aside.display()
                    ),
                    Err(err) => eprintln!(
                        "md: {} could not be read ({message}) nor moved aside ({err}); starting with no settings",
                        path.display()
                    ),
                }
                (Vec::new(), true)
            }
            Load::Unreadable(message) => {
                eprintln!("md: {} could not be read ({message}); settings stay in memory", path.display());
                (Vec::new(), false)
            }
        };
        KeyFileSettingsStore {
            path,
            values: RefCell::new(values),
            persist,
            warned: Cell::new(false),
            changed: Event::new(),
        }
    }

    fn value(&self, key: &str) -> Option<SettingValue> {
        self.values.borrow().iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn set(&self, key: &str, value: SettingValue) {
        {
            let mut values = self.values.borrow_mut();
            match values.iter_mut().find(|(k, _)| k == key) {
                Some((_, old)) if *old == value => return,
                Some((_, old)) => *old = value,
                None => values.push((key.to_string(), value)),
            }
        }
        self.save();
        // No borrow is held: a handler may read (or write) the store.
        self.changed.emit(key);
    }

    /// Writes every value; on failure the store keeps working in memory.
    fn save(&self) {
        if !self.persist {
            return;
        }
        let file = glib::KeyFile::new();
        for (key, value) in self.values.borrow().iter() {
            match value {
                SettingValue::Bool(b) => file.set_boolean(GROUP, key, *b),
                SettingValue::String(s) => file.set_string(GROUP, key, s),
            }
        }
        let written = match self.path.parent() {
            Some(folder) => std::fs::create_dir_all(folder).map_err(|err| err.to_string()),
            None => Ok(()),
        }
        .and_then(|()| file.save_to_file(&self.path).map_err(|err| err.to_string()));
        if let Err(message) = written
            && !self.warned.replace(true)
        {
            eprintln!("md: settings stay in memory: {} could not be written ({message})", self.path.display());
        }
    }
}

impl SettingsStore for KeyFileSettingsStore {
    fn get_string(&self, key: &str) -> Option<String> {
        match self.value(key) {
            Some(SettingValue::String(s)) => Some(s),
            _ => None,
        }
    }

    fn set_string(&self, key: &str, value: &str) {
        self.set(key, SettingValue::String(value.to_string()));
    }

    fn get_bool(&self, key: &str, fallback: bool) -> bool {
        match self.value(key) {
            Some(SettingValue::Bool(b)) => b,
            _ => fallback,
        }
    }

    fn set_bool(&self, key: &str, value: bool) {
        self.set(key, SettingValue::Bool(value));
    }

    fn remove(&self, key: &str) {
        let removed = {
            let mut values = self.values.borrow_mut();
            let before = values.len();
            values.retain(|(k, _)| k != key);
            values.len() != before
        };
        if removed {
            self.save();
            self.changed.emit(key);
        }
    }

    fn changed(&self) -> &Event<str> {
        &self.changed
    }
}

enum Load {
    Values(Vec<(String, SettingValue)>),
    Missing,
    Corrupt(String),
    Unreadable(String),
}

fn load(path: &Path) -> Load {
    // GLib calls anything but a regular file a *parse* error; it is not a corrupt settings file.
    if let Ok(metadata) = std::fs::metadata(path)
        && !metadata.is_file()
    {
        return Load::Unreadable(String::from("not a regular file"));
    }
    let file = glib::KeyFile::new();
    match file.load_from_file(path, glib::KeyFileFlags::NONE) {
        Ok(()) => Load::Values(read_values(&file)),
        // A file where the folder should be: nothing can exist at the path, so nothing is lost by
        // writing (which then fails and warns).
        Err(err) if err.matches(glib::FileError::Noent) || err.matches(glib::FileError::Notdir) => Load::Missing,
        Err(err) if err.kind::<glib::KeyFileError>().is_some() => Load::Corrupt(err.to_string()),
        Err(err) => Load::Unreadable(err.to_string()),
    }
}

/// The `[md]` group's values: the bool keys as booleans when they spell one, everything else as
/// strings. A key given twice keeps its last value in its first place; a value GLib cannot
/// unescape is skipped; localised keys (`key[de]`) are not md's and are skipped too.
fn read_values(file: &glib::KeyFile) -> Vec<(String, SettingValue)> {
    let Ok(keys) = file.keys(GROUP) else { return Vec::new() };
    let mut values: Vec<(String, SettingValue)> = Vec::new();
    for key in keys.iter() {
        let key = key.as_str();
        if key.contains('[') {
            continue;
        }
        let as_bool = if BOOL_KEYS.contains(&key) { file.boolean(GROUP, key).ok() } else { None };
        let value = match as_bool {
            Some(b) => SettingValue::Bool(b),
            None => match file.string(GROUP, key) {
                Ok(s) => SettingValue::String(s.to_string()),
                Err(_) => continue,
            },
        };
        match values.iter_mut().find(|(k, _)| k == key) {
            Some((_, old)) => *old = value,
            None => values.push((key.to_string(), value)),
        }
    }
    values
}

fn corrupt_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(CORRUPT_SUFFIX);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    /// A fresh folder under the system temp directory, removed when dropped.
    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new(name: &str) -> TempFolder {
            let dir = std::env::temp_dir().join(format!("md-settings-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp folder");
            TempFolder(dir)
        }

        fn settings(&self) -> PathBuf {
            self.0.join("md").join("settings.conf")
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// What the md-core view-mode codec writes, give or take: long, punctuated, one line.
    fn codec_like_value() -> String {
        let mut value = String::from("v1");
        for n in 0..200 {
            value.push_str(&format!(";{n:08x}=split|edit#[x]"));
        }
        value
    }

    #[test]
    fn values_survive_a_reopen_with_their_types_and_their_order() {
        let temp = TempFolder::new("round-trip");
        let store = KeyFileSettingsStore::open(temp.settings());
        let codec = codec_like_value();
        store.set_string(settings_keys::VIEW_MODE_MEMORY, &codec);
        store.set_string(settings_keys::PDF_PAGE_SIZE, "a5");
        store.set_string(settings_keys::BOOK_BOOKMARK, "/home/me/Книги/My Book ");
        store.set_bool(settings_keys::CONTINUE_LISTS, false);
        store.set_bool(settings_keys::WRAP_SELECTION, true);
        store.set_string(settings_keys::BOOK_LAST_ARTICLE, "");
        store.set_string(settings_keys::BOOK_VIEW_MODE, " \tleading\nand\\escapes\r");
        store.set_string(settings_keys::DOCUMENT_WINDOW_SIZE, "1200x800");
        store.remove(settings_keys::DOCUMENT_WINDOW_SIZE);
        assert!(temp.settings().is_file());

        let reopened = KeyFileSettingsStore::open(temp.settings());
        assert_eq!(reopened.get_string(settings_keys::VIEW_MODE_MEMORY).as_deref(), Some(codec.as_str()));
        assert_eq!(reopened.get_string(settings_keys::PDF_PAGE_SIZE).as_deref(), Some("a5"));
        assert_eq!(reopened.get_string(settings_keys::BOOK_BOOKMARK).as_deref(), Some("/home/me/Книги/My Book "));
        assert!(!reopened.get_bool(settings_keys::CONTINUE_LISTS, true));
        assert!(reopened.get_bool(settings_keys::WRAP_SELECTION, false));
        assert_eq!(reopened.get_string(settings_keys::BOOK_LAST_ARTICLE).as_deref(), Some(""));
        assert_eq!(reopened.get_string(settings_keys::BOOK_VIEW_MODE).as_deref(), Some(" \tleading\nand\\escapes\r"));
        assert_eq!(reopened.get_string(settings_keys::DOCUMENT_WINDOW_SIZE), None);
        let keys: Vec<String> = reopened.values.borrow().iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(
            keys,
            [
                settings_keys::VIEW_MODE_MEMORY,
                settings_keys::PDF_PAGE_SIZE,
                settings_keys::BOOK_BOOKMARK,
                settings_keys::CONTINUE_LISTS,
                settings_keys::WRAP_SELECTION,
                settings_keys::BOOK_LAST_ARTICLE,
                settings_keys::BOOK_VIEW_MODE,
            ]
        );

        let text = std::fs::read_to_string(temp.settings()).expect("settings.conf");
        assert!(text.starts_with("[md]\n"), "{text}");
        assert!(text.contains("\nmd.continueLists=false\n"), "{text}");
        assert!(text.contains("\nmd.pdfPageSize=a5\n"), "{text}");
    }

    #[test]
    fn a_string_read_of_a_bool_is_absent_and_the_reverse_too() {
        let temp = TempFolder::new("types");
        let store = KeyFileSettingsStore::open(temp.settings());
        store.set_bool(settings_keys::CAPITALIZE_SENTENCES, false);
        store.set_string(settings_keys::PDF_PAGE_SIZE, "true");
        store.set_string(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, "not a bool");
        for store in [store, KeyFileSettingsStore::open(temp.settings())] {
            assert_eq!(store.get_string(settings_keys::CAPITALIZE_SENTENCES), None);
            assert!(!store.get_bool(settings_keys::CAPITALIZE_SENTENCES, true));
            assert!(!store.get_bool(settings_keys::PDF_PAGE_SIZE, false));
            assert!(store.get_bool(settings_keys::PDF_PAGE_SIZE, true));
            assert_eq!(store.get_string(settings_keys::PDF_PAGE_SIZE).as_deref(), Some("true"));
            assert_eq!(store.get_string(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS).as_deref(), Some("not a bool"));
            assert!(store.get_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, true));
            assert_eq!(store.get_string("md.absent"), None);
            assert!(store.get_bool("md.absent", true));
        }
    }

    #[test]
    fn changed_fires_once_per_real_change_after_the_file_was_written() {
        let temp = TempFolder::new("changed");
        let store = Rc::new(KeyFileSettingsStore::open(temp.settings()));
        let seen: Rc<RefCell<Vec<(String, Option<String>)>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let (seen, path, weak) = (Rc::clone(&seen), temp.settings(), Rc::downgrade(&store));
            store.changed().connect(move |key: &str| {
                // On disk already, and the store can be read from inside the handler.
                let on_disk = KeyFileSettingsStore::open(path.clone()).get_string(key);
                let live = weak.upgrade().and_then(|s| s.get_string(key));
                assert_eq!(on_disk, live);
                seen.borrow_mut().push((key.to_string(), on_disk));
            });
        }
        store.set_string(settings_keys::PDF_PAGE_SIZE, "a4");
        store.set_string(settings_keys::PDF_PAGE_SIZE, "a4");
        store.set_string(settings_keys::PDF_PAGE_SIZE, "letter");
        store.set_bool(settings_keys::CONTINUE_LISTS, true);
        store.set_bool(settings_keys::CONTINUE_LISTS, true);
        store.remove(settings_keys::BOOK_BOOKMARK);
        store.remove(settings_keys::PDF_PAGE_SIZE);
        store.remove(settings_keys::PDF_PAGE_SIZE);
        // A bool replacing a string is a change.
        store.set_string(settings_keys::WRAP_SELECTION, "x");
        store.set_bool(settings_keys::WRAP_SELECTION, true);
        let keys: Vec<String> = seen.borrow().iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(
            keys,
            [
                settings_keys::PDF_PAGE_SIZE,
                settings_keys::PDF_PAGE_SIZE,
                settings_keys::CONTINUE_LISTS,
                settings_keys::PDF_PAGE_SIZE,
                settings_keys::WRAP_SELECTION,
                settings_keys::WRAP_SELECTION,
            ]
        );
        assert_eq!(seen.borrow()[0].1.as_deref(), Some("a4"));
        assert_eq!(seen.borrow()[1].1.as_deref(), Some("letter"));
        assert_eq!(seen.borrow()[3].1, None);
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_and_the_store_starts_empty() {
        let temp = TempFolder::new("corrupt");
        let path = temp.settings();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let garbage = b"this is not a key file\n[[[\n\xff\xfe\n";
        std::fs::write(&path, garbage).unwrap();

        let store = KeyFileSettingsStore::open(path.clone());
        assert_eq!(store.get_string(settings_keys::PDF_PAGE_SIZE), None);
        assert!(!path.exists());
        let aside = path.with_file_name("settings.conf.corrupt");
        assert_eq!(std::fs::read(&aside).unwrap(), garbage);

        store.set_string(settings_keys::PDF_PAGE_SIZE, "a5");
        let reopened = KeyFileSettingsStore::open(path);
        assert_eq!(reopened.get_string(settings_keys::PDF_PAGE_SIZE).as_deref(), Some("a5"));
        assert_eq!(std::fs::read(&aside).unwrap(), garbage);
    }

    #[test]
    fn a_file_without_the_md_group_is_an_empty_store_not_a_corrupt_one() {
        let temp = TempFolder::new("other-group");
        let path = temp.settings();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# hand-edited\n[other]\nkey=value\n").unwrap();
        let store = KeyFileSettingsStore::open(path.clone());
        assert_eq!(store.get_string("key"), None);
        assert!(path.is_file());
        assert!(!path.with_file_name("settings.conf.corrupt").exists());
    }

    #[test]
    fn an_unwritable_folder_keeps_the_store_in_memory() {
        let temp = TempFolder::new("unwritable");
        // A regular file where the folder should be: no one, root included, can create below it.
        let blocker = temp.0.join("md");
        std::fs::write(&blocker, "").unwrap();
        let store = KeyFileSettingsStore::open(temp.settings());
        let fired = Rc::new(Cell::new(0));
        {
            let fired = Rc::clone(&fired);
            store.changed().connect(move |_: &str| fired.set(fired.get() + 1));
        }
        store.set_string(settings_keys::PDF_PAGE_SIZE, "a5");
        store.set_bool(settings_keys::CONTINUE_LISTS, false);
        assert_eq!(store.get_string(settings_keys::PDF_PAGE_SIZE).as_deref(), Some("a5"));
        assert!(!store.get_bool(settings_keys::CONTINUE_LISTS, true));
        assert_eq!(fired.get(), 2);
        assert!(store.warned.get());
    }

    #[test]
    fn an_unreadable_file_is_never_overwritten() {
        let temp = TempFolder::new("unreadable");
        // A folder in the file's place cannot be read as a key file.
        std::fs::create_dir_all(temp.settings()).unwrap();
        let store = KeyFileSettingsStore::open(temp.settings());
        store.set_string(settings_keys::PDF_PAGE_SIZE, "a5");
        assert_eq!(store.get_string(settings_keys::PDF_PAGE_SIZE).as_deref(), Some("a5"));
        assert!(temp.settings().is_dir());
        assert!(!corrupt_path(&temp.settings()).exists());
    }
}
