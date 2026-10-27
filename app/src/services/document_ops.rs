//! Rename and Move To through `gio::File::move_` (shell-design §6.4) — it refuses an existing
//! target and copies + deletes across file systems — and the TextBundle / TextPack import helpers
//! (§6.5). Errors come back as the message an alert shows.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2

/// Renames the file in its folder; the new path, or the message.
pub fn rename(path: &str, new_file_name: &str) -> Result<String, String> {
    Err(String::from("S0 stub: document_ops::rename is work package P2"))
}

/// Moves the file into `folder`; the new path, or the message.
pub fn move_to(path: &str, folder: &str) -> Result<String, String> {
    Err(String::from("S0 stub: document_ops::move_to is work package P2"))
}
