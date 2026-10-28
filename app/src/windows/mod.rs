//! Windows (shell-design §1): the frozen [`host::WindowHost`] every window implements, the window
//! manager that owns them, the document window, `session.json` and the untitled drafts.
//!
//! | Module | What | Owner |
//! | --- | --- | --- |
//! | [`host`] | `WindowHost`, `WindowKind` — frozen | S0 |
//! | [`window_manager`] | live windows, routing, open / present / quit, the Window list | P1 |
//! | [`document_window`] | the document window | P1 |
//! | [`session`] | `session.json` through md-logic's `SessionStore` | P1 |
//! | [`drafts`] | untitled drafts (+ tests in a temp folder) | P1 |

pub mod document_window;
pub mod drafts;
pub mod host;
pub mod session;
pub mod window_manager;
