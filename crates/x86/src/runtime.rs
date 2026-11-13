//! Host execution imports and their adapters to architectural values.

mod ports;
mod segments;

use relocade_compiler::{BlockBuilder, BuildError, Func, Program, Table, TypedFunc, Val, I1, I32};

#[derive(Clone, Copy)]
pub(crate) struct Runtime {
    dispatch: Func,
    resolve_call: TypedFunc<fn() -> (u32, bool)>,
    functions: Table,
    interpret: Func,
    resolve_segment: Func,
    query_segment_descriptor: TypedFunc<crate::abi::QuerySegmentDescriptorSignature>,
    complete_segment_load: Func,
    ports: ports::Ports,
}

impl Runtime {
    pub(crate) fn declare(program: &mut Program) -> Self {
        let dispatch = program.import_function(crate::abi::DISPATCH);
        let interpret = program.import_function(crate::abi::INTERPRET);
        let resolve_segment = program.import_function(crate::abi::RESOLVE_SEGMENT);
        let query_segment_descriptor = crate::abi::QUERY_SEGMENT_DESCRIPTOR.import(program);
        Self {
            ports: ports::Ports::declare(program),
            dispatch,
            resolve_call: crate::abi::RESOLVE_CALL.import(program),
            functions: program.import_table(crate::abi::EXECUTION_FUNCTIONS),
            interpret,
            resolve_segment,
            query_segment_descriptor,
            complete_segment_load: program.import_function(crate::abi::COMPLETE_SEGMENT_LOAD),
        }
    }

    pub(crate) fn dispatch(self, body: BlockBuilder<'_>) -> Result<(), BuildError> {
        body.tail_call(self.dispatch, &[])
    }

    pub(crate) fn resolve_call(
        self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<(Val<I32>, Val<I1>), BuildError> {
        self.resolve_call.call(body, ())
    }

    pub(crate) fn enter(self, body: BlockBuilder<'_>, slot: Val<I32>) -> Result<(), BuildError> {
        body.tail_call_indirect(self.functions, slot, &[])
    }

    /// Enters ordinary execution at the already-published instruction boundary.
    pub(crate) fn interpret(self, body: BlockBuilder<'_>) -> Result<(), BuildError> {
        body.tail_call(self.interpret, &[])
    }
}
