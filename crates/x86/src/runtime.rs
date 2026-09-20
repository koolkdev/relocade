//! Host execution imports and their adapters to architectural values.

use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, FunctionImport, Program, Signature, Type, Val, I1, I16, I32,
};

use crate::{
    exception::{Exception, ExceptionVector},
    segment::SegmentValues,
    Segment, SegmentPermissions,
};

#[derive(Clone, Copy)]
pub(crate) struct Runtime {
    dispatch: Func,
    resolve_segment: Func,
    segment_permissions: Func,
}

impl Runtime {
    pub(crate) fn declare(program: &mut Program) -> Self {
        let dispatch = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "dispatch".into(),
            signature: Signature {
                parameters: vec![Type::I32],
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
        let segment_permissions = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "segmentPermissions".into(),
            signature: Signature {
                parameters: vec![Type::I16],
                results: vec![Type::I32],
            },
        });
        Self {
            dispatch,
            resolve_segment,
            segment_permissions,
        }
    }

    pub(crate) fn dispatch(
        self,
        body: FunctionBuilder<'_>,
        eip: &Val<I32>,
    ) -> Result<(), BuildError> {
        body.tail_call(self.dispatch, &[eip.into()])
    }

    /// Queries the current host descriptor view without loading a segment or
    /// accessing guest memory. The result has read/write rights in bits 0/1.
    pub(crate) fn segment_permissions(
        self,
        body: &mut FunctionBuilder<'_>,
        selector: &Val<I16>,
    ) -> Result<SegmentPermissions<Val<I1>>, BuildError> {
        let permissions = body.call::<I32>(self.segment_permissions, &[selector.into()])?;
        Ok(SegmentPermissions {
            readable: permissions.and(1).ne(0),
            writable: permissions.and(2).ne(0),
        })
    }

    /// The host resolves its descriptor view without inspecting or changing CPU
    /// state, guest RAM or page tables. Status zero succeeds; other statuses are
    /// segment-fault vectors. Cache fields are used only after the fault path has exited.
    pub(crate) fn resolve_segment(
        self,
        body: &mut FunctionBuilder<'_>,
        segment: Segment,
        selector: &Val<I16>,
        on_fault: impl Fn(FunctionBuilder<'_>, Exception<Val<I32>>) -> Result<(), BuildError>,
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
