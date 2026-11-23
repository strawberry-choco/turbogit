//! The bridge the plan pins: one owner, one revision signal, short borrows.
//! Never hold a borrow across a callback, an await, or a component body.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nsh_core::prelude::*;
use turbogit_app::state::AppState;

/// A mutation posted for the pump to run on the state.
type DeferredMutation = Box<dyn FnOnce(&mut AppState)>;

#[derive(Clone)]
pub struct AppHandle {
    state: Rc<RefCell<AppState>>,
    revision: Signal<u64>,
    /// Mutations an async task's tail posts instead of running inline: the
    /// task can resume while a binding still holds the state borrow, and a
    /// re-entrant `update` there would panic. The pump drains the queue.
    deferred: Rc<RefCell<Vec<DeferredMutation>>>,
}

impl PartialEq for AppHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

impl AppHandle {
    /// One handle per mounted root: the state lives with it, the revision
    /// signal tells bindings when a change landed.
    pub fn new(state: AppState) -> Self {
        AppHandle {
            state: Rc::new(RefCell::new(state)),
            revision: signal(0_u64),
            deferred: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Posts a mutation for the pump to run on the state: the way an async
    /// tail answers the state without borrowing against a live binding.
    pub fn defer(&self, f: impl FnOnce(&mut AppState) + 'static) {
        self.deferred.borrow_mut().push(Box::new(f));
    }

    pub fn read<R>(&self, f: impl FnOnce(&AppState) -> R) -> R {
        let _ = self.revision.get();
        f(&self.state.borrow())
    }

    /// Reads the state without subscribing the caller to changes: the one
    /// initial read a screen's body takes to seed a signal (the tab the
    /// shell mirrors), never a paint-path read.
    pub fn peek<R>(&self, f: impl FnOnce(&AppState) -> R) -> R {
        nsh_core::element::reactive::untracked(|| {
            let state = self.state.borrow();
            f(&state)
        })
    }

    pub fn update(&self, f: impl FnOnce(&mut AppState)) {
        self.update_with(f);
    }

    /// Like `update`, for a handler that reads a result out of the same
    /// change (a new selection position, for a scroll into view).
    pub fn update_with<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> R {
        let result;
        {
            let mut state = self.state.borrow_mut();
            result = f(&mut state);
        }
        self.revision.update(|n| *n += 1);
        result
    }

    /// Drains the event queue, runs the deferred mutations, and advances
    /// the poll tick; returns how long the next pump should wait.
    pub fn pump(&self) -> Option<Duration> {
        let deferred: Vec<_> = self.deferred.borrow_mut().drain(..).collect();
        let mut changed;
        let wait;
        {
            let mut state = self.state.borrow_mut();
            changed = state.drain_events() > 0;
            wait = state.tick_incoming_poll(Instant::now());
            for f in deferred {
                f(&mut state);
                changed = true;
            }
        }
        if changed {
            self.revision.update(|n| *n += 1);
        }
        wait
    }
}
