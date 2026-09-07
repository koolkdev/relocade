use wasm86_compiler::{Val, I32};

use crate::state::{Gpr32, State};

pub(super) fn mov32(state: &mut State, destination: Gpr32, source: &Val<I32>) {
    state.write_register(destination, source);
}
