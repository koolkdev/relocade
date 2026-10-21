//! Host execution imports and their adapters to architectural values.

mod ports;
mod segments;

use relocade_compiler::{BlockBuilder, BuildError, Func, Program};

#[derive(Clone, Copy)]
pub(crate) struct Runtime {
    dispatch: Func,
    interpret: Func,
    resolve_segment: Func,
    query_segment_descriptor: Func,
    complete_segment_load: Func,
    ports: ports::Ports,
}

impl Runtime {
    pub(crate) fn declare(program: &mut Program) -> Self {
        let dispatch = program.import_function(crate::abi::DISPATCH);
        let interpret = program.import_function(crate::abi::INTERPRET);
        let resolve_segment = program.import_function(crate::abi::RESOLVE_SEGMENT);
        let query_segment_descriptor =
            program.import_function(crate::abi::QUERY_SEGMENT_DESCRIPTOR);
        Self {
            ports: ports::Ports::declare(program),
            dispatch,
            interpret,
            resolve_segment,
            query_segment_descriptor,
            complete_segment_load: program.import_function(crate::abi::COMPLETE_SEGMENT_LOAD),
        }
    }

    pub(crate) fn dispatch(self, body: BlockBuilder<'_>) -> Result<(), BuildError> {
        body.tail_call(self.dispatch, &[])
    }

    /// Enters ordinary execution at the already-published instruction boundary.
    pub(crate) fn interpret(self, body: BlockBuilder<'_>) -> Result<(), BuildError> {
        body.tail_call(self.interpret, &[])
    }
}
