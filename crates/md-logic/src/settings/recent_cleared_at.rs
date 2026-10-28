//! `md.linux.recentClearedAt` (shell-design §6.6, §9.1): what File ▸ Open Recent ▸ Clear Menu means
//! when the recent list is the desktop's. md registers its documents in the shared
//! `recently-used.xbel` (`gtk::RecentManager`), which feeds GNOME Files' Recent and which other
//! applications keep their own entries in, so Clear Menu cannot delete anything: it records the
//! moment, and md's menu lists only the entries md registered after it. The shell reads the md
//! stamp of each entry (`RecentInfo::application_info("md")`, the time md last registered it) as
//! seconds since the Unix epoch and asks [`is_listed`].

use super::settings_keys;
use super::settings_store::SettingsStore;

/// The last Clear Menu, in seconds since the Unix epoch; `None` when md's menu was never cleared or
/// the stored value is not a whole number (a hand-edited settings file).
pub fn read(settings: &(impl SettingsStore + ?Sized)) -> Option<i64> {
    settings.get_string(settings_keys::RECENT_CLEARED_AT)?.parse().ok()
}

/// Clear Menu: remember `now_unix_seconds` as the moment md's recent history starts again.
pub fn clear(settings: &(impl SettingsStore + ?Sized), now_unix_seconds: i64) {
    settings.set_string(settings_keys::RECENT_CLEARED_AT, &now_unix_seconds.to_string());
}

/// Whether an entry md last registered at `stamp_unix_seconds` belongs in md's Open Recent: it was
/// registered strictly after the last Clear Menu (`cleared_at` from [`read`]), or the menu was
/// never cleared.
pub fn is_listed(stamp_unix_seconds: i64, cleared_at: Option<i64>) -> bool {
    cleared_at.is_none_or(|cleared| stamp_unix_seconds > cleared)
}

/// Whether a [`SettingsStore::changed`] key is this one (the shell rebuilds Open Recent).
pub fn is_key(key: &str) -> bool {
    key == settings_keys::RECENT_CLEARED_AT
}
