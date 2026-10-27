//! Port of md.win `Md.App.Logic.Activation`: what an activation asks for, and the routing table
//! that turns it into window-manager actions (shell-design.md §1.2).
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`activation_action`] | `ActivationAction.cs` |
//! | [`activation_description`] | `ActivationDescription.cs` (`ActivationKind`, `ActivationItem`) |
//! | [`activation_router`] | `ActivationRouter.cs` |
//!
//! How the GTK app feeds it: `activate` → [`ActivationDescription::for_launch`] with no
//! arguments; `open` → [`ActivationDescription::for_files`] with one
//! [`ActivationItem::from_location`] per `GFile` (its path, else its URI; `is_folder` from the file
//! type); `is_first` true only for the activation that started the process. Then
//! `activation_router::resolve(activation_router::route(&description), &registry, &identity)` and
//! execute the actions in order.

pub mod activation_action;
pub mod activation_description;
pub mod activation_router;

pub use activation_action::ActivationAction;
pub use activation_description::{ActivationDescription, ActivationItem, ActivationKind};
