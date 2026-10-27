//! Port of md.win `FakeAlerts`.

use std::cell::RefCell;
use std::fmt;

use super::answer_queue::AnswerQueue;
use crate::exec::{Completion, LocalBoxFuture};
use crate::seams::{Alerts, CloseChoice};

/// One [`Alerts::warn`] call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub title: String,
    pub message: String,
}

impl Warning {
    pub fn new(title: &str, message: &str) -> Self {
        Warning { title: title.to_string(), message: message.to_string() }
    }
}

/// One [`Alerts::prompt_name`] call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamePrompt {
    pub title: String,
    pub message: String,
    pub initial: String,
    pub accept_label: String,
}

impl NamePrompt {
    pub fn new(title: &str, message: &str, initial: &str, accept_label: &str) -> Self {
        NamePrompt {
            title: title.to_string(),
            message: message.to_string(),
            initial: initial.to_string(),
            accept_label: accept_label.to_string(),
        }
    }
}

/// One [`Alerts::confirm_delete`] call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmation {
    pub title: String,
    pub message: String,
}

impl Confirmation {
    pub fn new(title: &str, message: &str) -> Self {
        Confirmation { title: title.to_string(), message: message.to_string() }
    }
}

/// [`Alerts`] with scripted answers: queue what the user will click; every call is recorded. An
/// unscripted question gets the safe answer — the one that changes nothing (cancel / don't delete
/// / don't replace) — so a forgotten script cannot delete or overwrite anything in a test by
/// accident; assert on the recordings to catch it.
///
/// A dialog can also be held open: `hold_*` queues a held answer and returns its
/// [`Completion`]; the call is recorded as usual and its future stays pending until the test
/// completes it (then pump the executor). Held and immediate answers share one queue per dialog,
/// consumed in order.
#[derive(Default)]
pub struct FakeAlerts {
    warnings: RefCell<Vec<Warning>>,
    name_prompts: RefCell<Vec<NamePrompt>>,
    delete_confirmations: RefCell<Vec<Confirmation>>,
    save_changes_questions: RefCell<Vec<String>>,
    replace_confirmations: RefCell<Vec<String>>,

    warn_answers: AnswerQueue<()>,
    name_answers: AnswerQueue<Option<String>>,
    delete_answers: AnswerQueue<bool>,
    save_changes_answers: AnswerQueue<CloseChoice>,
    replace_answers: AnswerQueue<bool>,
}

impl fmt::Debug for FakeAlerts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeAlerts")
            .field("warnings", &self.warnings.borrow())
            .field("name_prompts", &self.name_prompts.borrow())
            .field("delete_confirmations", &self.delete_confirmations.borrow())
            .field("save_changes_questions", &self.save_changes_questions.borrow())
            .field("replace_confirmations", &self.replace_confirmations.borrow())
            .finish()
    }
}

impl FakeAlerts {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- recordings ----

    pub fn warnings(&self) -> Vec<Warning> {
        self.warnings.borrow().clone()
    }

    pub fn name_prompts(&self) -> Vec<NamePrompt> {
        self.name_prompts.borrow().clone()
    }

    pub fn delete_confirmations(&self) -> Vec<Confirmation> {
        self.delete_confirmations.borrow().clone()
    }

    /// The titles [`Alerts::ask_save_changes`] was asked with.
    pub fn save_changes_questions(&self) -> Vec<String> {
        self.save_changes_questions.borrow().clone()
    }

    /// The messages [`Alerts::confirm_replace`] was asked with.
    pub fn replace_confirmations(&self) -> Vec<String> {
        self.replace_confirmations.borrow().clone()
    }

    // ---- scripts ----

    /// The next name prompt answers `answer` (`None` = Cancel).
    pub fn queue_name_answer(&self, answer: Option<&str>) {
        self.name_answers.push(answer.map(str::to_string));
    }

    pub fn queue_delete_answer(&self, delete: bool) {
        self.delete_answers.push(delete);
    }

    pub fn queue_save_changes_answer(&self, choice: CloseChoice) {
        self.save_changes_answers.push(choice);
    }

    pub fn queue_replace_answer(&self, replace: bool) {
        self.replace_answers.push(replace);
    }

    /// The next warning stays up until the completion is completed (with `()`).
    pub fn hold_warn(&self) -> Completion<()> {
        self.warn_answers.hold()
    }

    pub fn hold_name(&self) -> Completion<Option<String>> {
        self.name_answers.hold()
    }

    pub fn hold_delete(&self) -> Completion<bool> {
        self.delete_answers.hold()
    }

    pub fn hold_save_changes(&self) -> Completion<CloseChoice> {
        self.save_changes_answers.hold()
    }

    pub fn hold_replace(&self) -> Completion<bool> {
        self.replace_answers.hold()
    }
}

impl Alerts for FakeAlerts {
    fn warn(&self, title: String, message: String) -> LocalBoxFuture<'static, ()> {
        self.warnings.borrow_mut().push(Warning { title, message });
        self.warn_answers.next_or(|| ())
    }

    fn prompt_name(
        &self,
        title: String,
        message: String,
        initial: String,
        accept_label: String,
    ) -> LocalBoxFuture<'static, Option<String>> {
        self.name_prompts.borrow_mut().push(NamePrompt { title, message, initial, accept_label });
        self.name_answers.next_or(|| None)
    }

    fn confirm_delete(&self, title: String, message: String) -> LocalBoxFuture<'static, bool> {
        self.delete_confirmations.borrow_mut().push(Confirmation { title, message });
        self.delete_answers.next_or(|| false)
    }

    fn ask_save_changes(&self, title: String) -> LocalBoxFuture<'static, CloseChoice> {
        self.save_changes_questions.borrow_mut().push(title);
        self.save_changes_answers.next_or(|| CloseChoice::Cancel)
    }

    fn confirm_replace(&self, message: String) -> LocalBoxFuture<'static, bool> {
        self.replace_confirmations.borrow_mut().push(message);
        self.replace_answers.next_or(|| false)
    }
}
