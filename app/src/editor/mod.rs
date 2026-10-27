//! The editor column and the panes around it (shell-design §3, §5.2–§5.4, §6.3).
//!
//! | Module | What |
//! | --- | --- |
//! | [`editor_pane`] | `EditorPane`: the `TextView`, placeholder, CSS class, jumps |
//! | [`typing`] | `TypingHooks` / `FormattingAdapter` glue at user-action boundaries |
//! | [`offsets`] | pure: char ↔ UTF-16 offsets (+ tests) |
//! | [`find_bar`] | `FindBar` |
//! | [`article_panes`] | `ArticlePanes`: editor column + preview, breakpoint, divider |
//! | [`zen`] | `ZenBin` (2/3 × 92 %), `ZenCapsule` |
//! | [`footer`] | `Footer`: word count, spinner |
//! | [`document_bar`] | the conflict / save-error bar |
//!
//! Owner: P3.

pub mod article_panes;
pub mod document_bar;
pub mod editor_pane;
pub mod find_bar;
pub mod footer;
pub mod offsets;
pub mod typing;
pub mod zen;
