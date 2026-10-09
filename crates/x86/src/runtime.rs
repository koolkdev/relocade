//! Host execution imports and their adapters to architectural values.

mod budget;
mod ports;

use wasm86_compiler::{
    BlockBuilder, BuildError, Func, FunctionImport, Program, Signature, Type, Val, I1, I16, I32,
};

use crate::{
    exception::{Exception, ExceptionVector},
    segment::SegmentValues,
    Segment, SegmentDescriptorInfo,
};

#[derive(Clone, Copy)]
pub(crate) struct Runtime {
    budget: Option<budget::Budget>,
    dispatch: Func,
    interpret: Func,
    resolve_segment: Func,
    query_segment_descriptor: Func,
    ports: ports::Ports,
}

impl Runtime {
    pub(crate) fn declare(program: &mut Program, execution_budget: bool) -> Self {
        let dispatch = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "dispatch".into(),
            signature: Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I64],
            },
        });
        let interpret = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "interpret".into(),
            signature: Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
        });
        let resolve_segment = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "resolveSegment".into(),
            signature: Signature {
                parameters: vec![Type::I32, Type::I16],
                results: vec![
                    Type::I32,
                    Type::I32,
                    Type::I32,
                    Type::I32,
                    Type::I16,
                    Type::I16,
                ],
            },
        });
        let query_segment_descriptor = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "querySegmentDescriptor".into(),
            signature: Signature {
                parameters: vec![Type::I16],
                results: vec![Type::I1, Type::I1, Type::I1, Type::I32, Type::I32],
            },
        });
        Self {
            budget: execution_budget.then(|| budget::Budget::declare(program)),
            ports: ports::Ports::declare(program),
            dispatch,
            interpret,
            resolve_segment,
            query_segment_descriptor,
        }
    }

    pub(crate) fn check_budget(
        self,
        body: &mut BlockBuilder<'_>,
        exhausted: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        if let Some(budget) = self.budget {
            let remaining = budget.remaining(body)?;
            body.if_(remaining.eq(0), exhausted)?;
        }
        Ok(())
    }

    pub(crate) fn is_budgeted(self) -> bool {
        self.budget.is_some()
    }

    pub(crate) fn remaining_work(
        self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Option<Val<I32>>, BuildError> {
        self.budget.map(|budget| budget.remaining(body)).transpose()
    }

    pub(crate) fn publish_work(
        self,
        body: &mut BlockBuilder<'_>,
        remaining: Option<&Val<I32>>,
    ) -> Result<(), BuildError> {
        if let (Some(budget), Some(remaining)) = (self.budget, remaining) {
            budget.publish(body, remaining)?;
        }
        Ok(())
    }

    pub(crate) fn dispatch(self, body: BlockBuilder<'_>, eip: &Val<I32>) -> Result<(), BuildError> {
        body.tail_call(self.dispatch, &[eip.into()])
    }

    /// Enters ordinary execution at the already-published instruction boundary.
    pub(crate) fn interpret(self, body: BlockBuilder<'_>) -> Result<(), BuildError> {
        body.tail_call(self.interpret, &[])
    }

    /// Queries the current host descriptor view without loading a segment or
    /// accessing guest memory.
    pub(crate) fn query_segment_descriptor(
        self,
        body: &mut BlockBuilder<'_>,
        selector: &Val<I16>,
    ) -> Result<SegmentDescriptorInfo<Val<I1>, Val<I32>>, BuildError> {
        let (visible, readable, writable, access_rights, limit) =
            body.call::<(I1, I1, I1, I32, I32)>(self.query_segment_descriptor, &[selector.into()])?;
        Ok(SegmentDescriptorInfo {
            visible,
            readable,
            writable,
            access_rights,
            limit,
        })
    }

    /// The host resolves its descriptor view without inspecting or changing CPU
    /// state, guest RAM or page tables. Status zero succeeds; other statuses are
    /// segment-fault vectors. Cache fields are used only after the fault path has exited.
    pub(crate) fn resolve_segment(
        self,
        body: &mut BlockBuilder<'_>,
        segment: Segment,
        selector: &Val<I16>,
        on_fault: impl Fn(BlockBuilder<'_>, Exception<Val<I32>>) -> Result<(), BuildError>,
    ) -> Result<SegmentValues, BuildError> {
        let (status, error_code, base, limit, selector, attributes) =
            body.call::<(I32, I32, I32, I32, I16, I16)>(
                self.resolve_segment,
                &[(segment as u32).into(), selector.into()],
            )?;
        body.if_(status.ne(0), |mut fault| {
            let vectors = [
                ExceptionVector::SegmentNotPresent as u32,
                ExceptionVector::StackFault as u32,
                ExceptionVector::GeneralProtection as u32,
            ];
            fault.switch(&status, &vectors, |arm, vector| {
                let error_code = error_code.clone();
                let exception = match vector {
                    Some(vector) if vector == ExceptionVector::SegmentNotPresent as u32 => {
                        Exception::SegmentNotPresent { error_code }
                    }
                    Some(vector) if vector == ExceptionVector::StackFault as u32 => {
                        Exception::StackFault { error_code }
                    }
                    Some(vector) if vector == ExceptionVector::GeneralProtection as u32 => {
                        Exception::GeneralProtection { error_code }
                    }
                    _ => return arm.trap(),
                };
                on_fault(arm, exception)
            })?;
            fault.trap()
        })?;
        Ok(SegmentValues {
            base,
            limit,
            selector,
            attributes,
        })
    }
}
