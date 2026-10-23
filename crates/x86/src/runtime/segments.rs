//! Descriptor imports, immediately decoded into named architectural values.

use super::Runtime;
use crate::{
    segment::SegmentValues, ExceptionValues, ExceptionVector, Segment, SegmentDescriptorInfoValues,
};
use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I16};

pub(crate) struct ResolvedValues {
    pub(crate) cache: SegmentValues,
    pub(crate) needs_completion: Val<I1>,
}

impl Runtime {
    /// Queries the current host descriptor view without loading a segment or
    /// accessing guest memory.
    pub(crate) fn query_segment_descriptor(
        self,
        body: &mut BlockBuilder<'_>,
        selector: &Val<I16>,
    ) -> Result<SegmentDescriptorInfoValues, BuildError> {
        self.query_segment_descriptor.call(body, selector)
    }

    /// The host reads only canonical table registers and dedicated descriptor
    /// bytes. Other CPU fields need not be published. Resolution does not change
    /// state; status zero succeeds and other statuses are segment-fault vectors.
    pub(crate) fn resolve_segment(
        self,
        body: &mut BlockBuilder<'_>,
        segment: Segment,
        selector: &Val<I16>,
        on_fault: impl Fn(BlockBuilder<'_>, ExceptionValues) -> Result<(), BuildError>,
    ) -> Result<ResolvedValues, BuildError> {
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
        ) = body.call::<crate::abi::ResolveSegmentResults>(
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
        body.if_(needs_completion, |mut commit| {
            commit.call::<()>(self.complete_segment_load, &[selector.into()])?;
            Ok(())
        })
    }
}
