//! Reads of segment fields that remain fixed until the entry dispatches.

use std::mem::size_of;

use relocade_compiler::{BlockBuilder, BuildError, Val, ValueType, I1, I16, I32};

use crate::{
    segment::{Segment, SegmentSelection},
    ExecutionProfile, SegmentDefaultSize, SegmentProfile,
};

use super::{access::cpu_load, Cpu, StoredSegment};

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
    fn new(profile: ExecutionProfile, segment: Segment) -> Self {
        let mut cache = Self::default();
        match profile {
            ExecutionProfile::Real16 => {
                cache.limit = Some(0xffffu32.into());
                cache.big = Some(false.into());
            }
            ExecutionProfile::Protected(SegmentProfile::Flat32)
                if matches!(
                    segment,
                    Segment::Cs | Segment::Ds | Segment::Es | Segment::Ss
                ) =>
            {
                cache.base = Some(0.into());
                cache.limit = Some(u32::MAX.into());
                cache.usable = Some(true.into());
                cache.readable = Some(true.into());
                cache.writable = Some((segment != Segment::Cs).into());
                cache.executable = Some((segment == Segment::Cs).into());
                cache.expand_down = Some(false.into());
                if segment == Segment::Ss {
                    cache.big = Some(true.into());
                }
            }
            ExecutionProfile::Protected(_) => {}
        }
        if segment == Segment::Cs {
            cache.big = Some((profile.code_default_size() == SegmentDefaultSize::Bits32).into());
        }
        cache
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
                    at: index.mul(size_of::<StoredSegment>() as u32))?;
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

    fn cache(&mut self, segment: Segment) -> &mut SegmentReadCache {
        self.cached[segment as usize]
            .get_or_insert_with(|| SegmentReadCache::new(self.profile, segment))
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
            let field = field(self.cache(Segment::ALL[index as usize]));
            if let Some(value) = field {
                return Ok(value.clone());
            }
            let value = load(body, (index as u32).into())?;
            *field = Some(value.clone());
            return Ok(value);
        }

        // Address defaults can reuse constants common to DS and SS.
        if segment.is_ds_or_ss() {
            let ds = field(self.cache(Segment::Ds)).clone();
            let ss = field(self.cache(Segment::Ss)).clone();
            if let (Some(ds), Some(ss)) = (ds, ss) {
                if ds.same_expression(&ss) {
                    return Ok(ds);
                }
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
