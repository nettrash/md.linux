//! Port of md.win `FakePickers`.

use std::cell::RefCell;
use std::fmt;

use super::answer_queue::AnswerQueue;
use crate::exec::{Completion, LocalBoxFuture};
use crate::seams::{FileTypeChoice, Pickers};

/// One [`Pickers::save_file`] call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveCall {
    pub suggested_name: String,
    pub choices: Vec<FileTypeChoice>,
    pub default_extension: String,
}

impl SaveCall {
    /// The labels of the type choices, in order.
    pub fn labels(&self) -> Vec<String> {
        self.choices.iter().map(|c| c.label.clone()).collect()
    }
}

/// [`Pickers`] with scripted results; every call is recorded with its arguments, so a test can pin
/// the suggested name, the type choices and their order. Unscripted = the user cancelled (empty
/// list / `None`). `hold_*` keeps a dialog open until the test completes it, as on
/// [`FakeAlerts`](super::FakeAlerts).
#[derive(Default)]
pub struct FakePickers {
    open_calls: RefCell<Vec<Vec<String>>>,
    save_calls: RefCell<Vec<SaveCall>>,
    folder_calls: RefCell<Vec<Option<String>>>,

    open_answers: AnswerQueue<Vec<String>>,
    save_answers: AnswerQueue<Option<String>>,
    folder_answers: AnswerQueue<Option<String>>,
}

impl fmt::Debug for FakePickers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakePickers")
            .field("open_calls", &self.open_calls.borrow())
            .field("save_calls", &self.save_calls.borrow())
            .field("folder_calls", &self.folder_calls.borrow())
            .finish()
    }
}

impl FakePickers {
    pub fn new() -> Self {
        Self::default()
    }

    /// The extension lists [`Pickers::open_files`] was called with.
    pub fn open_calls(&self) -> Vec<Vec<String>> {
        self.open_calls.borrow().clone()
    }

    pub fn save_calls(&self) -> Vec<SaveCall> {
        self.save_calls.borrow().clone()
    }

    /// The titles [`Pickers::pick_folder`] was called with.
    pub fn folder_calls(&self) -> Vec<Option<String>> {
        self.folder_calls.borrow().clone()
    }

    /// The next open dialog returns these paths (empty = cancelled).
    pub fn queue_open_answer(&self, paths: &[&str]) {
        self.open_answers.push(paths.iter().map(|p| p.to_string()).collect());
    }

    /// The next save dialog returns this path (`None` = cancelled).
    pub fn queue_save_answer(&self, path: Option<&str>) {
        self.save_answers.push(path.map(str::to_string));
    }

    /// The next folder dialog returns this path (`None` = cancelled).
    pub fn queue_folder_answer(&self, path: Option<&str>) {
        self.folder_answers.push(path.map(str::to_string));
    }

    pub fn hold_open(&self) -> Completion<Vec<String>> {
        self.open_answers.hold()
    }

    pub fn hold_save(&self) -> Completion<Option<String>> {
        self.save_answers.hold()
    }

    pub fn hold_folder(&self) -> Completion<Option<String>> {
        self.folder_answers.hold()
    }
}

impl Pickers for FakePickers {
    fn open_files(&self, extensions: Vec<String>) -> LocalBoxFuture<'static, Vec<String>> {
        self.open_calls.borrow_mut().push(extensions);
        self.open_answers.next_or(Vec::new)
    }

    fn save_file(
        &self,
        suggested_name: String,
        choices: Vec<FileTypeChoice>,
        default_extension: String,
    ) -> LocalBoxFuture<'static, Option<String>> {
        self.save_calls.borrow_mut().push(SaveCall { suggested_name, choices, default_extension });
        self.save_answers.next_or(|| None)
    }

    fn pick_folder(&self, title: Option<String>) -> LocalBoxFuture<'static, Option<String>> {
        self.folder_calls.borrow_mut().push(title);
        self.folder_answers.next_or(|| None)
    }
}
