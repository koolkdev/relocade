//! Descriptor imports, immediately decoded into named architectural values.

use super::Runtime;
use crate::{
    segment::SegmentValues, ExceptionValues, ExceptionVector, Segment, SegmentDescriptorInfoValues,
};
use relocade_compiler::{BlockBuilder, BuildError, Func, Program, TypedFunc, Val, I1, I16, I32};

#[derive(Clone, Copy)]
pub(super) struct HostedDescriptors {
    resolve: Func,
    query: TypedFunc<crate::abi::QuerySegmentDescriptorSignature>,
    complete_load: Func,
}

impl HostedDescriptors {
    fn declare(program: &mut Program) -> Self {
        Self {
            resolve: program.import_function(crate::abi::RESOLVE_SEGMENT),
            query: crate::abi::QUERY_SEGMENT_DESCRIPTOR.import(program),
            complete_load: program.import_function(crate::abi::COMPLETE_SEGMENT_LOAD),
        }
    }
}

pub(crate) struct ResolvedValues {
    pub(crate) cache: SegmentValues,
    pub(crate) needs_completion: Val<I1>,
}

impl Runtime {
    pub(crate) fn import_hosted_descriptors(&mut self, program: &mut Program) {
        self.descriptors = Some(HostedDescriptors::declare(program));
    }

    pub(crate) fn supports_protected_descriptors(self) -> bool {
        self.descriptors.is_some()
    }

    /// Queries the current host descriptor view without loading a segment or
    /// accessing guest memory.
    pub(crate) fn query_segment_descriptor(
        self,
        body: &mut BlockBuilder<'_>,
        selector: &Val<I16>,
    ) -> Result<SegmentDescriptorInfoValues, BuildError> {
        self.descriptors
            .expect("descriptor queries require the hosted service")
            .query
            .call(body, selector)
    }

    /// The host reads only canonical table registers and dedicated descriptor
    /// bytes. Other CPU fields need not be published. Resolution does not change
    /// state; status zero succeeds and other statuses are segment-fault vectors.
    pub(crate) fn resolve_segment(
        self,
        body: &mut BlockBuilder<'_>,
        protected: Val<I1>,
        segment: Segment,
        selector: &Val<I16>,
        on_fault: impl Fn(BlockBuilder<'_>, ExceptionValues) -> Result<(), BuildError>,
    ) -> Result<ResolvedValues, BuildError> {
        let real_mode = || {
            let cache = crate::SegmentRegister::real_mode(segment, 0);
            (
                0u32.into(), // success
                0u32.into(), // error code
                selector.unsigned().extend::<I32>().shl(4),
                cache.limit.into(),
                selector.clone(),
                (cache.usable != 0).into(),
                (cache.readable != 0).into(),
                (cache.writable != 0).into(),
                (cache.executable != 0).into(),
                (cache.expand_down != 0).into(),
                (cache.big != 0).into(),
                false.into(), // real mode has no descriptor access bit to complete
            )
        };
        let (
            status,
            error_code,
            base,
            limit,
            selector,
            usable,
            readable,
            writable,
            executable,
            expand_down,
            big,
            needs_completion,
        ) = match self.descriptors {
            Some(descriptors) => body.if_value::<crate::abi::ResolveSegmentResults>(
                protected,
                |mut protected| {
                    let resolved = protected.call::<crate::abi::ResolveSegmentResults>(
                        descriptors.resolve,
                        &[(segment as u32).into(), selector.into()],
                    )?;
                    protected.yield_(resolved)
                },
                |real| real.yield_(real_mode()),
            )?,
            // Admission already stopped protected descriptor loads before their operands.
            None => real_mode(),
        };
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
                        ExceptionValues::segment_not_present(error_code)
                    }
                    Some(vector) if vector == ExceptionVector::StackFault as u32 => {
                        ExceptionValues::stack_fault(error_code)
                    }
                    Some(vector) if vector == ExceptionVector::GeneralProtection as u32 => {
                        ExceptionValues::general_protection(error_code)
                    }
                    _ => return arm.trap(),
                };
                on_fault(arm, exception)
            })?;
            fault.trap()
        })?;
        Ok(ResolvedValues {
            cache: SegmentValues {
                base,
                limit,
                selector,
                usable,
                readable,
                writable,
                executable,
                expand_down,
                big,
            },
            needs_completion,
        })
    }
    pub(crate) fn complete_segment_load(
        self,
        body: &mut BlockBuilder<'_>,
        selector: &Val<I16>,
        needs_completion: Val<I1>,
    ) -> Result<(), BuildError> {
        let Some(descriptors) = self.descriptors else {
            return Ok(());
        };
        body.if_(needs_completion, |mut commit| {
            commit.call::<()>(descriptors.complete_load, &[selector.into()])?;
            Ok(())
        })
    }
}
