use turbogit_domain::model::RootId;
use turbogit_services::branch_names::Prefix;

use crate::state::NewBranchBase;

/// Everything one opening of the New Branch dialog knows. One owner, replaced
/// whole on open, so an invocation cannot inherit a character or a start point
/// from the last one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewBranchSession {
    /// The repository this invocation is about, read at open rather than at
    /// click. `None` is a real state: the welcome screen has no root.
    pub root: Option<RootId>,
    /// The name field's buffer, seeded with the current branch. The kit widget
    /// writes the reader's typing straight into it, so there is one copy of the
    /// buffer in the process and the reducer reads it from `self`.
    pub name: String,
    pub checkout: bool,
    /// Where the branch starts, decided by the invoking action. The dialog reads
    /// it to state it and never writes it.
    pub start: NewBranchBase,
    pub suggest: SuggestState,
    /// Bumped by `open`, never reused. It salts the name field's egui id, so a
    /// previous invocation's caret, undo history and text offset belong to a
    /// widget that no longer exists.
    pub generation: u64,
}

/// The suggester's whole stored state. Both fields are keyed by a candidate's
/// text, never by an index, because the list is rebuilt under them every frame
/// and an index would arm a different row than the one the reader pressed Down
/// onto.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuggestState {
    /// The row Enter, Tab or a click would accept. Pruned, not cleared, whenever
    /// it is no longer in the table.
    pub armed: Option<String>,
    /// The live text Escape folded the list at. Equal to the text means folded;
    /// any other text means the fold has already been answered by typing.
    pub folded_at: Option<String>,
}

/// What the frame knew before the reducer ran. Built by the dialog body, which
/// is the only place that touches egui, so the reducer stays pure and testable
/// without a harness.
pub struct NameFrame<'a> {
    /// The text the suggester is answering, `None` when the field is unfocused
    /// or the whole buffer is selected. Derived from the field's cursor range,
    /// never stored.
    pub live_text: Option<&'a str>,
    pub candidates: &'a [Prefix],
    pub keys: NameKeys,
    pub create_clicked: bool,
    pub cancel_clicked: bool,
    pub clicked_candidate: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NameKeys {
    pub up: bool,
    pub down: bool,
    pub tab: bool,
    pub enter: bool,
    pub escape: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogReply {
    Nothing,
    Create,
    Close,
}

impl NewBranchSession {
    /// The only way to hold a session. No field is read from the previous one.
    pub fn open(
        generation: u64,
        root: Option<RootId>,
        prefill: Option<String>,
        start: NewBranchBase,
        checkout: bool,
    ) -> Self {
        Self {
            root,
            name: prefill.unwrap_or_default(),
            checkout,
            start,
            suggest: SuggestState::default(),
            generation,
        }
    }

    /// One entry point, called once per frame after every widget has painted.
    /// Returns what the dialog must do, never doing it.
    pub fn reduce(&mut self, frame: &NameFrame<'_>) -> DialogReply {
        let live = frame.live_text.unwrap_or_default();
        let showing = show_list(frame.live_text, frame.candidates, &self.suggest);

        if let Some(text) = frame.clicked_candidate {
            return self.accept(text);
        }
        if frame.keys.escape {
            if showing {
                self.suggest.folded_at = Some(live.to_owned());
                self.suggest.armed = None;
            } else {
                return DialogReply::Close;
            }
        }
        if (frame.keys.down || frame.keys.up) && showing {
            self.suggest.armed = advance(
                self.suggest.armed.as_deref(),
                frame.keys.down,
                frame.candidates,
            );
        }
        // Not showing: the keys are still taken by the dialog and do nothing.
        // The surface behind it reads the same `ArrowDown` out of the shared
        // input state, so taking them is what stops the Branches tree from
        // moving its selection under an open dialog.
        if frame.keys.tab && showing {
            let armed = self.suggest.armed.clone();
            match armed {
                Some(text) => return self.accept(&text),
                None => self.suggest.armed = frame.candidates.first().map(|p| p.text.clone()),
            }
        }
        // Prune, not sync: an armed row that left the list stops existing here, so
        // Enter can never accept a candidate that is no longer on screen.
        self.suggest.armed = self
            .suggest
            .armed
            .take()
            .filter(|text| showing && frame.candidates.iter().any(|p| p.text == *text));
        if frame.keys.enter {
            // Enter cannot bypass the gate the footer button wears.
            if self.name.trim().is_empty() {
                return DialogReply::Nothing;
            }
            let armed = self.suggest.armed.clone();
            if let Some(text) = armed {
                return self.accept(&text);
            }
            return DialogReply::Create;
        }
        if frame.create_clicked {
            return DialogReply::Create;
        }
        if frame.cancel_clicked {
            return DialogReply::Close;
        }
        DialogReply::Nothing
    }

    /// Accepting a candidate replaces the buffer with the candidate and drops the
    /// row, which is what makes the list close itself: the candidate no longer
    /// strictly extends the buffer, so it is filtered out of the next frame's
    /// table.
    fn accept(&mut self, candidate: &str) -> DialogReply {
        self.name = candidate.to_owned();
        self.suggest = SuggestState::default();
        DialogReply::Nothing
    }
}

/// Whether the list paints, derived every frame from three facts the reducer is
/// handed.
pub fn show_list(live: Option<&str>, candidates: &[Prefix], suggest: &SuggestState) -> bool {
    live.is_some_and(|text| {
        !text.is_empty() && !candidates.is_empty() && suggest.folded_at.as_deref() != Some(text)
    })
}

/// The highlight's next position, by candidate text. Wraps at both ends, and an
/// unarmed list is armed at the first row by `Down` and at the last by `Up`. An
/// armed text that is not in the list is treated as unarmed, so `Down` from a
/// pruned row restarts at the top.
pub fn advance(armed: Option<&str>, down: bool, candidates: &[Prefix]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let current = armed.and_then(|text| candidates.iter().position(|p| p.text == text));
    let next = match (current, down) {
        (Some(i), true) => (i + 1) % candidates.len(),
        (Some(i), false) => (i + candidates.len() - 1) % candidates.len(),
        (None, true) => 0,
        (None, false) => candidates.len() - 1,
    };
    Some(candidates[next].text.clone())
}
