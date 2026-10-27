//! Port of md.win `Md.App.Logic.Text`: the editor's typing (SmartTyping's two hooks and the state
//! machine behind them), the formatting commands' editor half, the Go ▸ Notes label and the
//! footer's word count.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`smart_typing_adapter`] | `SmartTypingAdapter.cs` (`SmartTypingAdapter`, `CapitalizationPlan`) |
//! | [`typing_hooks`] | `TypingHooks.cs` (`TypingHooks`, `TextEdit`, `DeleteDirection`, `CapitalTracker`) |
//! | [`formatting_adapter`] | `FormattingAdapter.cs` (`FormattingAdapter`; `HeadingChordGuard` not ported) |
//! | [`note_preview`] | `NotePreview.cs` |
//! | [`word_counter`] | `SimpleWordCounter.cs` and `IcuWordCounter.cs` (one counter on Linux) |
//!
//! Every offset these modules take or return is a UTF-16 code unit (md-core's editing rules and
//! the shared vectors count in them); the GTK shell converts `TextIter` char offsets with
//! `md_core::text::utf16`. The editor's buffer holds LF-only text, so nothing here swaps line
//! breaks (md.win swaps LF and CR for WinUI's `TextBox`).

pub mod formatting_adapter;
pub mod note_preview;
pub mod smart_typing_adapter;
pub mod typing_hooks;
pub mod word_counter;

pub use smart_typing_adapter::CapitalizationPlan;
pub use typing_hooks::{CapitalTracker, DeleteDirection, TextEdit, TypingHooks};
pub use word_counter::WritingStatsWordCounter;
