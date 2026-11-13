//! Reads of segment fields that remain fixed until the entry dispatches.

use std::mem::{offset_of, size_of};

use relocade_compiler::{BlockBuilder, BuildError, Val, ValueType, I1, I16, I32};

use crate::{
    segment::{Segment, SegmentSelection},
    ExecutionProfile,
};

use super::{access::cpu_load, Cpu, CpuState, SegmentRegister, StateAssumption};

/// Segment loads commit at the end of an entry. Cloning these read caches keeps
/// child-scoped loads local to their branch; inherited values remain reusable.
#[derive(Clone)]
pub(crate) struct SegmentReads<'cpu> {
    cpu: &'cpu Cpu,
    profile: ExecutionProfile,
    cached: [Option<SegmentReadCache>; 6],
}

#[derive(Clone, Default)]
struct SegmentReadCache {
    base: Option<Val<I32>>,
    limit: Option<Val<I32>>,
    selector: Option<Val<I16>>,
    usable: Option<Val<I1>>,
    readable: Option<Val<I1>>,
    writable: Option<Val<I1>>,
    executable: Option<Val<I1>>,
    expand_down: Option<Val<I1>>,
    big: Option<Val<I1>>,
}

impl SegmentReadCache {
    fn new(
        body: &mut BlockBuilder<'_>,
        cpu: &Cpu,
        profile: ExecutionProfile,
        segment: Segment,
    ) -> Result<Self, BuildError> {
        let mut cache = Self::default();
        let mut condition = Val::<I1>::from(true);
        let assumptions = StateAssumption::segment(profile, segment);
        let displacement = segment as u32 * size_of::<SegmentRegister>() as u32;
        macro_rules! assume_field {
            ($field:ident $(, $convert:ident($argument:expr))?) => {
                let offset = offset_of!(CpuState, segments.es.$field) as u32 + displacement;
                if let Some(assumption) = assumptions.iter().find(|assumption| assumption.offset == offset) {
                    let value = cpu_load!(body, cpu.memory(), segments.es.$field,
                        at: displacement)?;
                    condition = condition.and(assumption.predicate(&value));
                    cache.$field = Some(value$(.$convert($argument))?);
                }
            };
        }
        assume_field!(base);
        assume_field!(limit);
        assume_field!(usable, ne(0));
        assume_field!(readable, ne(0));
        assume_field!(writable, ne(0));
        assume_field!(executable, ne(0));
        assume_field!(expand_down, ne(0));
        assume_field!(big, ne(0));
        body.assume(condition)?;
        Ok(cache)
    }
}

macro_rules! read_field {
    ($name:ident, $field:ident, $ty:ty $(, $convert:ident($argument:expr))?) => {
        pub(crate) fn $name(
            &mut self,
            body: &mut BlockBuilder<'_>,
            segment: &SegmentSelection,
        ) -> Result<Val<$ty>, BuildError> {
            let memory = self.cpu.memory();
            self.read_field(body, segment, |cache| &mut cache.$field, |body, index| {
                let value = cpu_load!(body, memory, segments.es.$field,
                    at: index.mul(size_of::<SegmentRegister>() as u32))?;
                Ok(value$(.$convert($argument))?)
            })
        }
    };
}

impl<'cpu> SegmentReads<'cpu> {
    pub(crate) fn new(cpu: &'cpu Cpu, profile: ExecutionProfile) -> Self {
        Self {
            cpu,
            profile,
            cached: std::array::from_fn(|_| None),
        }
    }

    pub(crate) fn profile(&self) -> ExecutionProfile {
        self.profile
    }

    fn cache(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: Segment,
    ) -> Result<&mut SegmentReadCache, BuildError> {
        let cache = &mut self.cached[segment as usize];
        if cache.is_none() {
            *cache = Some(SegmentReadCache::new(
                body,
                self.cpu,
                self.profile,
                segment,
            )?);
        }
        Ok(cache.as_mut().unwrap())
    }

    fn read_field<T: ValueType>(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        field: impl Fn(&mut SegmentReadCache) -> &mut Option<Val<T>>,
        load: impl FnOnce(&mut BlockBuilder<'_>, Val<I32>) -> Result<Val<T>, BuildError>,
    ) -> Result<Val<T>, BuildError> {
        let index = segment.index();
        if let Some(index) = body.constant_bits(&index)? {
            let field = field(self.cache(body, Segment::ALL[index as usize])?);
            if let Some(value) = field {
                return Ok(value.clone());
            }
            let value = load(body, (index as u32).into())?;
            *field = Some(value.clone());
            return Ok(value);
        }

        // The compiler can simplify common DS/SS guarantees on these values.
        if segment.is_ds_or_ss() {
            let ds = field(self.cache(body, Segment::Ds)?).clone();
            let ss = field(self.cache(body, Segment::Ss)?).clone();
            if let (Some(ds), Some(ss)) = (ds, ss) {
                return Ok(index.eq(Segment::Ds as u32).select(ds, ss));
            }
        }
        load(body, index)
    }

    read_field!(base, base, I32);
    read_field!(limit, limit, I32);
    read_field!(read_selector, selector, I16);
    read_field!(usable, usable, I1, ne(0));
    read_field!(readable, readable, I1, ne(0));
    read_field!(writable, writable, I1, ne(0));
    read_field!(executable, executable, I1, ne(0));
    read_field!(expand_down, expand_down, I1, ne(0));
    read_field!(big, big, I1, ne(0));
}
