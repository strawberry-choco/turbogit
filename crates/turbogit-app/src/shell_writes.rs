//! The writes a tool window asks for that dispatch an `Operation`.
//!
//! The Shell's git interfaces are *dispatch an Operation* and *read a cached
//! value* (ADR-0020). This module owns the first half's **presentation side**:
//! the git verbs a pane, a popup or a dialog used to spell as an
//! `Operation::custom` closure it built at the click. Each one is a named entry
//! point here that takes the plain inputs the closure captured and returns the
//! `Operation` it owns, so the label, the
//! [`Affected`](crate::root_caches::Affected) scope, the guard-free
//! shape and the worker are decided in one file. It is the mirror image of
//! [`crate::shell_reads`], which does the same for the reads that are not
//! cached values.
//!
//! It does **not** cover the named [`Operation`](crate::operation::Operation)
//! variants a surface may construct directly — a
//! [`Operation::Fetch`](crate::operation::Operation::Fetch) computes its own
//! label, scope and worker, so there is nothing left to decide — and it does not
//! cover the app-side operations that own an identity of their own
//! (`dispatch_pull`, `dispatch_push`, the
//! [`Operation::Resolve`](crate::operation::Operation::Resolve) lifecycle
//! verbs). Those stay in [`crate::state`] because what they do after the work
//! lands is part of what they are.
//!
//! One sentence per load-bearing difference between the call sites these
//! entries came from:
//!
//! - **`Affected` is passed, never recomputed.** The branches pane and the
//!   sidebar choose the scope — a single root's or the whole fleet's — and
//!   [`Affected::All`](crate::root_caches::Affected::All) and
//!   `Affected::Root(_)` are not interchangeable, so an entry that derived its
//!   own would quietly narrow a fleet operation or widen a single-root one.
//! - **A label is a string the user reads in a toast**, so it is spelled at the
//!   entry rather than left to the caller: two entries that both say
//!   `Checkout {name}` must not be able to drift into two different strings.
//! - **Where two surfaces say the same verb, they share one entry** (the
//!   conflict resolver's `Apply merge resolution` and the conflicts pane's) so
//!   the copies that can disagree are gone. Where two surfaces say the same
//!   words but do different work — the two stash pops, one over a dialog's
//!   index `0` and one over a commit window's index — they stay two entries.

pub mod branch;
pub mod conflict;
pub mod remote;
