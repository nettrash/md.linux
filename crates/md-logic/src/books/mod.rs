//! Port of md.win `Md.App.Logic.Books`: the book window's logic (§8) over md-core's `book` module
//! (`book_model`, `book_folder`, `renumber_plan`, `book_compiler`, the flush hub).
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`book_library_model`] | `BookLibraryModel.cs` |
//! | [`book_navigator_model`] | `BookNavigatorModel.cs` (`IBookArticleSession` → [`BookArticleSession`], `TextFileSessionArticles`) |
//! | [`book_output`] | `BookOutput.cs` (`IBookOutputs` → [`BookOutputs`], `BookFlushGateBinding`; + [`PipelineBookOutputs`], md.win's App-side `BookExportOutputs`) |
//! | [`book_sidebar_model`] | `BookSidebarModel.cs` (`BookRowKind`, `BookRow`, `BookRowCommands`) |
//! | [`book_stepper`] | `BookStepper.cs` |
//!
//! How a book export reaches the files the editor holds: the book window owns a
//! [`BookFlushGateBinding`] on the app's one `BookFlushHub` whose `flush` is its article session's
//! `flush_now(false)`. Every [`BookOutput`] call posts to the hub *synchronously* before it lists
//! or reads anything; a failed save vetoes and the output stops without a word (the footer already
//! shows the save error). Then the book is listed and read through `plain_text_codec`, compiled
//! (PDF, Print) or kept structured (EPUB, LaTeX), and handed to [`BookOutputs`] — the window's
//! [`ExportPipeline`](crate::export::ExportPipeline) through [`PipelineBookOutputs`].

pub mod book_library_model;
pub mod book_navigator_model;
pub mod book_output;
pub mod book_sidebar_model;
pub mod book_stepper;

pub use book_library_model::BookLibraryModel;
pub use book_navigator_model::{BookArticleSession, BookNavigatorModel, TextFileSessionArticles};
pub use book_output::{BookFlushGateBinding, BookOutput, BookOutputs, PipelineBookOutputs};
pub use book_sidebar_model::{BookRow, BookRowCommands, BookRowKind};
pub use book_stepper::BookStepper;
