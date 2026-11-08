//! The mail engine behind Chord: Gmail sign-in, sync into a local SQLite
//! cache, actions with undo, rendering, and composing. Nothing in here knows
//! about a window, so the CLI, the service, and the window share exactly the
//! same behavior.

pub mod auth;
pub mod compose;
pub mod config;
pub mod gmail;
pub mod ops;
pub mod paths;
pub mod render;
pub mod service;
pub mod store;
pub mod sync;
pub mod text;
pub mod when;

pub use gmail::{Gmail, MailApi};
pub use reqwest;
pub use store::{MessageRow, Store, ThreadRow};
pub use sync::{SyncKind, SyncReport, sync};
