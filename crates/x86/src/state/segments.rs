//! Segment fields retain state values until a segment-changing entry dispatches.
use super::{ExecutionContext, SegmentRegister, Segments, State};
use crate::{
    segment::{Segment, SegmentSelection},
    ssa::{Location, SsaType},
};
use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I8};
use std::mem::{offset_of, size_of};

impl State<'_> {
    fn read_field<T: SsaType>(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        offset: usize,
    ) -> Result<Val<T>, BuildError> {
        let location = |segment| {
            Location::<T>::new(
                (offset_of!(ExecutionContext, cpu) + Segments::offset(segment) + offset) as u32,
            )
        };
        let index = segment.index();
        if let Some(index) = body.constant_bits(&index)? {
            return self
                .fields
                .read(body, location(Segment::ALL[index as usize]));
        }
        // A default DS/SS choice can share a value established for both fields.
        if segment.is_ds_or_ss() {
            let ds = self.fields.read(body, location(Segment::Ds))?;
            let ss = self.fields.read(body, location(Segment::Ss))?;
            if ds.same_expression(&ss) {
                return Ok(ds);
            }
        }
        let stride = size_of::<SegmentRegister>() as u32;
        let last_segment_offset = (Segment::ALL.len() - 1) as u32 * stride;
        self.fields.read(
            body,
            Location::indexed(
                (offset_of!(ExecutionContext, cpu) + Segments::offset(Segment::Es) + offset) as u32,
                last_segment_offset + T::BYTES,
                index.mul(stride),
            ),
        )
    }
}

macro_rules! read_field {
    ($name:ident, $stored:ty, $ty:ty $(, $convert:ident($argument:expr))?) => {
        impl State<'_> {
            pub(crate) fn $name(
                &mut self,
                body: &mut BlockBuilder<'_>,
                segment: &SegmentSelection,
            ) -> Result<Val<$ty>, BuildError> {
                let value = self.read_field::<$stored>(
                    body, segment, offset_of!(SegmentRegister, $name),
                )?;
                Ok(value$(.$convert($argument))?)
            }
        }
    };
}
read_field!(base, I32, I32);
read_field!(limit, I32, I32);
read_field!(usable, I8, I1, ne(0));
read_field!(readable, I8, I1, ne(0));
read_field!(writable, I8, I1, ne(0));
read_field!(executable, I8, I1, ne(0));
read_field!(expand_down, I8, I1, ne(0));
read_field!(big, I8, I1, ne(0));

impl State<'_> {
    pub(crate) fn read_selector(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
    ) -> Result<Val<I16>, BuildError> {
        self.read_field(body, segment, offset_of!(SegmentRegister, selector))
    }
}
