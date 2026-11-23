//! The callback shapes the controls' props carry, one alias each so the
//! props read as their role rather than their signature.

/// A change handler: the next value the app owns.
pub type OnChange<T> = Box<dyn Fn(&T) + 'static>;
