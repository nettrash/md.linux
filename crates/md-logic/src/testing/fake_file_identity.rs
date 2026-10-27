//! Port of md.win `FakeFileIdentity`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;

use crate::seams::FileIdentity;

/// [`FileIdentity`] without a file system: the canonical form is the path with its spelling
/// tidied lexically — doubled `/` and `.` segments dropped, `..` collapsed, no trailing `/` — unless
/// [`FakeFileIdentity::alias`] mapped that spelling to another: the way to model "two spellings of
/// one file" (a symlink, a bind mount) in a test.
///
/// LINUX DECISION: md.win's fake trims whitespace, turns `\` into `/` and matches aliases ignoring
/// case — all three are spelling differences on Windows and different files on Linux. The lexical
/// tidy-up is Linux's equivalent: those spellings name one file there, and the app's identity
/// (`std::fs::canonicalize`, i.e. `realpath`) folds them too — it resolves `..` after following
/// symlinks, which a test models with an alias. A relative path is left relative.
#[derive(Default)]
pub struct FakeFileIdentity {
    aliases: RefCell<HashMap<String, String>>,
    calls: RefCell<Vec<String>>,
}

impl fmt::Debug for FakeFileIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeFileIdentity").field("aliases", &self.aliases.borrow()).finish()
    }
}

impl FakeFileIdentity {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every path [`FileIdentity::canonical`] was asked about, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// `spelling` (after the tidy-up) is the file `canonical` names.
    pub fn alias(&self, spelling: &str, canonical: &str) {
        self.aliases.borrow_mut().insert(Self::normalise(spelling), Self::normalise(canonical));
    }

    /// The lexical tidy-up [`FileIdentity::canonical`] applies before the alias lookup.
    pub fn normalise(path: &str) -> String {
        let absolute = path.starts_with('/');
        let mut parts: Vec<&str> = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.last().is_some_and(|p| *p != "..") {
                        parts.pop();
                    } else if !absolute {
                        parts.push("..");
                    }
                }
                name => parts.push(name),
            }
        }
        let joined = parts.join("/");
        if absolute {
            format!("/{joined}")
        } else if joined.is_empty() {
            ".".to_string()
        } else {
            joined
        }
    }
}

impl FileIdentity for FakeFileIdentity {
    fn canonical(&self, path: &str) -> String {
        self.calls.borrow_mut().push(path.to_string());
        let n = Self::normalise(path);
        self.aliases.borrow().get(&n).cloned().unwrap_or(n)
    }
}
