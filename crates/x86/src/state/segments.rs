//! Reads of segment fields that remain fixed until the entry dispatches.

use std::mem::size_of;

use relocade_compiler::{BlockBuilder, BuildError, IntType, Val, I1, I16, I32};

use crate::{
    profile::ProfileFacts,
    segment::{Segment, SegmentAssumptions, SegmentSelection},
    ExecutionProfile, SegmentDefaultSize, SegmentFact,
};

use super::{access::cpu_load, Cpu, SegmentRegister};

/// Segment loads commit at the end of an entry. Cloning these read caches keeps
/// child-scoped loads local to their branch; inherited values remain reusable.
#[derive(Clone)]
pub(crate) struct SegmentReads<'cpu> {
    cpu: &'cpu Cpu,
    facts: ProfileFacts,
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
            ExecutionProfile::Protected(profile) => {
                let facts = SegmentAssumptions::for_profile(profile, segment);
                cache.base = facts.base.map(Into::into);
                cache.limit = facts.limit.map(Into::into);
                cache.usable = facts.usable.map(|value| (value != 0).into());
                cache.readable = facts.readable.map(|value| (value != 0).into());
                cache.writable = facts.writable.map(|value| (value != 0).into());
                cache.executable = facts.executable.map(|value| (value != 0).into());
                cache.expand_down = facts.expand_down.map(|value| (value != 0).into());
                cache.big = facts.big.map(|value| (value != 0).into());
            }
        }
        cache
    }
}

macro_rules! read_field {
    ($name:ident, $field:ident, $ty:ty, $fact:expr $(, $convert:ident($argument:expr))?) => {
        pub(crate) fn $name(
            &mut self,
            body: &mut BlockBuilder<'_>,
            segment: &SegmentSelection,
        ) -> Result<Val<$ty>, BuildError> {
            let memory = self.cpu.memory();
            self.read_field(body, segment, |cache| &mut cache.$field, $fact, |body, index| {
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
            facts: ProfileFacts::untracked(profile),
            cached: std::array::from_fn(|_| None),
        }
    }

    pub(crate) fn with_facts(mut self, facts: ProfileFacts) -> Self {
        self.facts = facts;
        self
    }

    pub(crate) fn facts(&self) -> &ProfileFacts {
        &self.facts
    }

    pub(crate) fn profile(&self) -> ExecutionProfile {
        self.facts.profile()
    }

    fn cache(&mut self, segment: Segment) -> &mut SegmentReadCache {
        let profile = self.facts.profile();
        self.cached[segment as usize].get_or_insert_with(|| SegmentReadCache::new(profile, segment))
    }

    fn read_field<T: IntType>(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        field: impl Fn(&mut SegmentReadCache) -> &mut Option<Val<T>>,
        fact: impl Fn(u64) -> Option<SegmentFact>,
        load: impl FnOnce(&mut BlockBuilder<'_>, Val<I32>) -> Result<Val<T>, BuildError>,
    ) -> Result<Val<T>, BuildError> {
        let index = segment.index();
        if let Some(index) = body.constant_bits(&index)? {
            let segment = Segment::ALL[index as usize];
            if let Some(value) = field(self.cache(segment)).clone() {
                if let Some(fact) = body.constant_bits(&value)?.and_then(&fact) {
                    self.facts.segment(segment, fact);
                }
                return Ok(value);
            }
            let value = load(body, (index as u32).into())?;
            *field(self.cache(segment)) = Some(value.clone());
            return Ok(value);
        }

        // Address defaults can reuse constants common to DS and SS.
        if segment.is_ds_or_ss() {
            let ds = field(self.cache(Segment::Ds)).clone();
            let ss = field(self.cache(Segment::Ss)).clone();
            if let (Some(ds), Some(ss)) = (ds, ss) {
                if ds.same_expression(&ss) {
                    if let Some(fact) = body.constant_bits(&ds)?.and_then(&fact) {
                        self.facts.segment(Segment::Ds, fact);
                        self.facts.segment(Segment::Ss, fact);
                    }
                    return Ok(ds);
                }
            }
        }
        load(body, index)
    }

    read_field!(base, base, I32, |v| Some(SegmentFact::Base(v as u32)));
    read_field!(limit, limit, I32, |v| Some(SegmentFact::Limit(v as u32)));
    read_field!(read_selector, selector, I16, |_| None);
    read_field!(
        usable,
        usable,
        I1,
        |v| Some(SegmentFact::Usable(v != 0)),
        ne(0)
    );
    read_field!(
        readable,
        readable,
        I1,
        |v| Some(SegmentFact::Readable(v != 0)),
        ne(0)
    );
    read_field!(
        writable,
        writable,
        I1,
        |v| Some(SegmentFact::Writable(v != 0)),
        ne(0)
    );
    read_field!(
        executable,
        executable,
        I1,
        |v| Some(SegmentFact::Executable(v != 0)),
        ne(0)
    );
    read_field!(
        expand_down,
        expand_down,
        I1,
        |v| Some(SegmentFact::ExpandDown(v != 0)),
        ne(0)
    );
    read_field!(
        big,
        big,
        I1,
        |v| Some(SegmentFact::DefaultSize(if v != 0 {
            SegmentDefaultSize::Bits32
        } else {
            SegmentDefaultSize::Bits16
        })),
        ne(0)
    );
}
