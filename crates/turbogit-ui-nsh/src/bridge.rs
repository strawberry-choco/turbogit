//! The bridge the plan pins: one owner, one revision signal, short borrows.
//! Never hold a borrow across a callback, an await, or a component body.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nsh_core::prelude::*;
use turbogit_app::state::AppState;

#[derive(Clone)]
pub struct AppHandle {
    state: Rc<RefCell<AppState>>,
    revision: Signal<u64>,
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
        }
    }

    pub fn read<R>(&self, f: impl FnOnce(&AppState) -> R) -> R {
        let _ = self.revision.get();
        f(&self.state.borrow())
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

    /// Drains the event queue and advances the poll tick; returns how long
    /// the next pump should wait.
    pub fn pump(&self) -> Option<Duration> {
        let changed;
        let wait;
        {
            let mut state = self.state.borrow_mut();
            changed = state.drain_events() > 0;
            wait = state.tick_incoming_poll(Instant::now());
        }
        if changed {
            self.revision.update(|n| *n += 1);
        }
        wait
    }
}
