//! Track SSA values and retain memory-backed state fields until publication.

mod state_fields;
mod tracked_value;

pub(super) use state_fields::{Location, SsaType, StateFields};
pub(super) use tracked_value::TrackedValue;

#[cfg(test)]
mod tests;
