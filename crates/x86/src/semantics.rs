use wasm86_compiler::{BuildError, IntoOp, I32};

use crate::state::{Gpr32, State};

pub(super) fn mov32(
    state: &mut State<'_, '_>,
    destination: Gpr32,
    source: impl IntoOp<I32>,
) -> Result<(), BuildError> {
    state.write_register(destination, source)
}
