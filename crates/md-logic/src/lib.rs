//! md's shell logic — the port of md.win's `Md.App.Logic`: everything the GTK app does that can
//! be tested without a display. No GTK, GLib or WebKit type appears in this crate; the app
//! implements [`seams`] and drives the futures on GLib's main context.

pub mod activation;
pub mod books;
pub mod commands;
pub mod documents;
pub mod event;
pub mod exec;
pub mod export;
pub mod preview;
pub mod seams;
pub mod settings;
pub mod strings;
pub mod text;
pub mod view;
pub mod windows;
