//! Port of md.win `Activation/ActivationDescription.cs` (`ActivationKind`, `ActivationItem`,
//! `ActivationDescription`), mapped onto `GApplication`.
//!
//! LINUX DECISIONS — how GTK hands md an activation, and what that makes of md.win's rows:
//! - `GApplication::activate` (a launch with nothing to open: the first run, or `md` run again
//!   while md is running, which D-Bus forwards to the primary instance) is
//!   [`ActivationKind::Launch`] with no arguments.
//! - `GApplication::open` (`HANDLES_OPEN`: paths on the command line, a file manager's "Open
//!   With", the `.desktop` file's `Exec=md %U`) is [`ActivationKind::File`]. GLib has already
//!   turned each argument into a `GFile`, resolving a relative path against the *invoking*
//!   process's working directory; the app builds one [`ActivationItem`] per `GFile` from its local
//!   path (or, when it has none, its URI) and asks it whether it is a folder.
//! - Arguments, when the app routes a launch with arguments itself, are argv tokens the shell has
//!   already split: there is no Windows command-line string to split by `CommandLineToArgvW`'s
//!   rules, no executable token to drop (argv\[0\] is never passed), and only `-` starts a switch —
//!   `/` starts every absolute path.
//! - A location may be a path or a URI. A `file://` URI becomes its path; any other scheme names
//!   nothing md can edit in place and is dropped.
//! - The kinds are `Launch`, `File` and `Other`: Linux has no protocol or startup-task
//!   activation of its own (an autostart entry is a plain launch), and anything a future
//!   `GApplication` entry point adds routes as a launch, as md.win's `Other` does.

/// The activation kinds md answers, named without GTK. Anything but [`ActivationKind::File`]
/// routes as a launch (§1.2's last row).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActivationKind {
    Launch,
    File,
    Other,
}

/// One item of a file activation: the path, and whether it is a folder. The folder flag is the
/// app's answer to `g_file_query_file_type` — it cannot be inferred from the name, and a
/// `.textbundle` is a folder whose name has an extension (§6.5).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ActivationItem {
    pub path: String,
    pub is_folder: bool,
}

impl ActivationItem {
    pub fn file(path: &str) -> Self {
        ActivationItem { path: path.to_string(), is_folder: false }
    }

    pub fn folder(path: &str) -> Self {
        ActivationItem { path: path.to_string(), is_folder: true }
    }

    /// An item from what `GApplication` hands over: a local path as is, a `file://` URI as its
    /// path (percent-decoded), and `None` for a URI of any other scheme (`sftp://`, `trash:///`,
    /// …) or for an empty location.
    pub fn from_location(location: &str, is_folder: bool) -> Option<Self> {
        let path = path_from_location(location)?;
        Some(ActivationItem { path, is_folder })
    }
}

/// The OS-free description of one activation, built by the app from `GApplication`'s `activate`
/// or `open` and consumed by [`activation_router`](super::activation_router) (§1.2). Everything the routing rules need is a
/// field here, so the rules are pure.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ActivationDescription {
    /// The activation kind; anything but [`ActivationKind::File`] routes as a launch.
    pub kind: ActivationKind,
    /// File-activation items, in the order `GApplication` gave them.
    pub items: Vec<ActivationItem>,
    /// Launch arguments: argv after the program name, already split by the shell.
    pub arguments: Vec<String>,
    /// True for the activation that started the process, false for one forwarded to the primary
    /// instance by a second `md`. Only the first may restore a session; a forwarded launch that
    /// carries nothing means "the user asked for md again", which is a new untitled window (§1.2).
    pub is_first: bool,
    /// `session.json` lists at least one saved document that still exists. Computed by the app
    /// with `session_store::restorable` before routing, so the router never touches a disk.
    pub has_restorable_session: bool,
}

impl ActivationDescription {
    pub fn for_launch(arguments: &[&str], is_first: bool, has_restorable_session: bool) -> Self {
        ActivationDescription {
            kind: ActivationKind::Launch,
            items: Vec::new(),
            arguments: arguments.iter().map(|a| a.to_string()).collect(),
            is_first,
            has_restorable_session,
        }
    }

    pub fn for_files(items: Vec<ActivationItem>, is_first: bool, has_restorable_session: bool) -> Self {
        ActivationDescription {
            kind: ActivationKind::File,
            items,
            arguments: Vec::new(),
            is_first,
            has_restorable_session,
        }
    }

    /// Any other activation: §1.2's "anything else" row, routed as a launch.
    pub fn for_other(kind: ActivationKind, is_first: bool, has_restorable_session: bool) -> Self {
        ActivationDescription { kind, items: Vec::new(), arguments: Vec::new(), is_first, has_restorable_session }
    }

    /// The launch arguments as file paths. A switch — any token starting with `-` — makes the
    /// whole command line a switch invocation, not a document list, so there are none: treating
    /// the rest as documents would open a switch's operand (an output folder, say) as a file.
    /// Every other token is a location ([`ActivationItem::from_location`]): a `file://` URI
    /// becomes its path, another URI is dropped, and spaces, quotes and backslashes are ordinary
    /// characters of the one token they are in.
    pub fn argument_paths(&self) -> Vec<String> {
        let mut paths = Vec::with_capacity(self.arguments.len());
        for token in &self.arguments {
            if token.is_empty() {
                continue;
            }
            if token.starts_with('-') {
                return Vec::new();
            }
            if let Some(path) = path_from_location(token) {
                paths.push(path);
            }
        }
        paths
    }
}

/// See [`ActivationItem::from_location`].
fn path_from_location(location: &str) -> Option<String> {
    if location.is_empty() {
        return None;
    }
    if has_prefix_ignoring_ascii_case(location, "file:") {
        return path_from_file_uri(location);
    }
    if is_uri(location) {
        return None;
    }
    Some(location.to_string())
}

/// RFC 3986's `scheme "://"`: a letter, then letters, digits, `+`, `-` or `.`, then `://`. Only
/// the authority form counts: a file named `notes:v2.md` is a file, not a URI.
fn is_uri(location: &str) -> bool {
    let Some(colon) = location.find("://") else { return false };
    let scheme = &location.as_bytes()[..colon];
    !scheme.is_empty()
        && scheme[0].is_ascii_alphabetic()
        && scheme.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// `g_filename_from_uri` for `file:///p`, `file://localhost/p` and `file:/p`: the absolute path,
/// percent-decoded. `None` for another host (a file on another machine is not a local path), a
/// malformed escape, a decoded NUL or bytes that are not UTF-8 (md-logic's paths are strings).
fn path_from_file_uri(uri: &str) -> Option<String> {
    let rest = &uri["file:".len()..];
    let path = match rest.strip_prefix("//") {
        Some(authority_and_path) => {
            let slash = authority_and_path.find('/')?;
            let host = &authority_and_path[..slash];
            if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                return None;
            }
            &authority_and_path[slash..]
        }
        None if rest.starts_with('/') => rest,
        None => return None,
    };
    let decoded = percent_decode(path)?;
    if decoded.contains('\0') { None } else { Some(decoded) }
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn has_prefix_ignoring_ascii_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len() && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}
