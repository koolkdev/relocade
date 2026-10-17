//! Retain state fields in SSA and synchronize overlapping memory accesses.

use super::TrackedValue;
use std::marker::PhantomData;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, MemoryType, Val, I16, I32, I64, I8, V128};

#[derive(Clone)]
pub(crate) struct Location<T: SsaType> {
    address: Address,
    marker: PhantomData<T>,
}

#[derive(Clone)]
enum Address {
    Fixed(u32),
    Indexed { span: Span, displacement: Val<I32> },
}

impl<T: SsaType> Location<T> {
    pub(crate) fn new(offset: u32) -> Self {
        Self {
            address: Address::Fixed(offset),
            marker: PhantomData,
        }
    }

    /// The range beginning at `base` must cover every possible accessed byte.
    /// `displacement` is relative to that base, including any subfield offset.
    pub(crate) fn indexed(base: u32, bytes: u32, displacement: Val<I32>) -> Self {
        Self {
            address: Address::Indexed {
                span: Span::new(base, bytes),
                displacement,
            },
            marker: PhantomData,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct LocationKey {
    offset: u32,
    bytes: u32,
}

impl LocationKey {
    fn span(self) -> Span {
        Span::new(self.offset, self.bytes)
    }
}

#[derive(Clone, Copy)]
struct Span {
    start: u64,
    end: u64,
}

impl Span {
    fn new(offset: u32, bytes: u32) -> Self {
        Self {
            start: u64::from(offset),
            end: u64::from(offset) + u64::from(bytes),
        }
    }

    fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    fn covers(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

/// Values retain their logical type; stores discard bits beyond the location width.
#[derive(Clone)]
pub(crate) enum DefinitionValue {
    Byte(TrackedValue<I8>),
    Word(TrackedValue<I16>),
    Dword(TrackedValue<I32>),
    Qword(TrackedValue<I64>),
    Vector(TrackedValue<V128>),
}

impl DefinitionValue {
    fn is_dirty(&self) -> bool {
        match self {
            Self::Byte(value) => value.dirty_value().is_some(),
            Self::Word(value) => value.dirty_value().is_some(),
            Self::Dword(value) => value.dirty_value().is_some(),
            Self::Qword(value) => value.dirty_value().is_some(),
            Self::Vector(value) => value.dirty_value().is_some(),
        }
    }

    fn mark_clean(&mut self) {
        match self {
            Self::Byte(value) => value.mark_clean(),
            Self::Word(value) => value.mark_clean(),
            Self::Dword(value) => value.mark_clean(),
            Self::Qword(value) => value.mark_clean(),
            Self::Vector(value) => value.mark_clean(),
        }
    }

    fn store(
        &self,
        body: &mut BlockBuilder<'_>,
        memory: Mem,
        base: &Val<I32>,
        offset: u32,
    ) -> Result<(), BuildError> {
        match self {
            Self::Byte(value) => body.store_at(memory, base, offset, value.value()),
            Self::Word(value) => body.store_at(memory, base, offset, value.value()),
            Self::Dword(value) => body.store_at(memory, base, offset, value.value()),
            Self::Qword(value) => body.store_at(memory, base, offset, value.value()),
            Self::Vector(value) => body.store_at(memory, base, offset, value.value()),
        }
    }
}

pub(crate) trait SsaType: MemoryType {
    fn retain(value: TrackedValue<Self>) -> DefinitionValue;
    fn retained(definition: &DefinitionValue) -> &TrackedValue<Self>;
    fn retained_mut(definition: &mut DefinitionValue) -> &mut TrackedValue<Self>;
}

macro_rules! ssa_type {
    ($ty:ty, $variant:ident) => {
        impl SsaType for $ty {
            fn retain(value: TrackedValue<Self>) -> DefinitionValue {
                DefinitionValue::$variant(value)
            }

            fn retained(definition: &DefinitionValue) -> &TrackedValue<Self> {
                let DefinitionValue::$variant(value) = definition else {
                    unreachable!("a location's width determines its retained value type")
                };
                value
            }

            fn retained_mut(definition: &mut DefinitionValue) -> &mut TrackedValue<Self> {
                let DefinitionValue::$variant(value) = definition else {
                    unreachable!("a location's width determines its retained value type")
                };
                value
            }
        }
    };
}

ssa_type!(I8, Byte);
ssa_type!(I16, Word);
ssa_type!(I32, Dword);
ssa_type!(I64, Qword);
ssa_type!(V128, Vector);

#[derive(Clone)]
struct Definition {
    location: LocationKey,
    value: DefinitionValue,
    first_write: Option<usize>,
}

/// Retains the current SSA values of typed fields in one backing memory.
/// Reads and definitions belong to one straight-line build path; terminal
/// publication can target its descendant arms.
/// Direct writes represent completed effects, not speculative changes to undo.
/// Every backing write overlapping managed locations must pass through this owner.
/// Cloning forks the retained definitions for a descendant path; it copies no memory.
#[derive(Clone)]
pub(crate) struct StateFields {
    memory: Mem,
    base: Val<I32>,
    // Live fixed definitions never overlap; an exact replacement needs no flush.
    definitions: Vec<Definition>,
    writes: usize,
}

impl StateFields {
    pub(crate) fn new(memory: Mem) -> Self {
        Self::with_base(memory, 0.into())
    }

    /// Captures an address base. Field offsets and alias spans are relative to this
    /// immutable base; independently managed records must not overlap.
    pub(crate) fn with_base(memory: Mem, base: Val<I32>) -> Self {
        Self {
            memory,
            base,
            definitions: Vec::new(),
            writes: 0,
        }
    }

    pub(crate) fn read<T: SsaType>(
        &mut self,
        body: &mut BlockBuilder<'_>,
        location: Location<T>,
    ) -> Result<Val<T>, BuildError> {
        let offset = match location.address {
            Address::Fixed(offset) => offset,
            Address::Indexed { span, displacement } => {
                let displacement = body.value(self.base.add(displacement))?;
                self.flush(body, span, false)?;
                return body.load_at::<T>(self.memory, displacement, span.start as u32);
            }
        };
        let location = LocationKey {
            offset,
            bytes: T::BYTES,
        };
        if let Some(entry) = self
            .definitions
            .iter()
            .find(|entry| entry.location == location)
        {
            return T::retained(&entry.value).read(body);
        }
        self.flush(body, location.span(), false)?;
        self.definitions
            .retain(|entry| !location.span().overlaps(entry.location.span()));
        let value = body.load_at::<T>(self.memory, &self.base, location.offset)?;
        self.definitions.push(Definition {
            location,
            value: T::retain(TrackedValue::new(body, &value)?),
            first_write: None,
        });
        Ok(value)
    }

    pub(crate) fn define<T: SsaType>(
        &mut self,
        body: &mut BlockBuilder<'_>,
        location: Location<T>,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        let offset = match location.address {
            Address::Fixed(offset) => offset,
            Address::Indexed { span, displacement } => {
                let displacement = body.value(self.base.add(displacement))?;
                let value = body.value(value)?;
                self.flush(body, span, false)?;
                body.store_at(self.memory, displacement, span.start as u32, value)?;
                self.definitions
                    .retain(|entry| !span.overlaps(entry.location.span()));
                return Ok(());
            }
        };
        let value = body.value(value)?;
        let location = LocationKey {
            offset,
            bytes: T::BYTES,
        };
        if let Some(entry) = self
            .definitions
            .iter_mut()
            .find(|entry| entry.location == location)
        {
            if T::retained_mut(&mut entry.value).define(body, value)? && entry.first_write.is_none()
            {
                entry.first_write = Some(self.writes);
                self.writes += 1;
            }
            return Ok(());
        }
        let span = location.span();
        // A covering definition replaces every byte of the older value. Partial
        // overlap must first preserve its remaining bytes in backing memory.
        self.flush(body, span, true)?;
        self.definitions
            .retain(|entry| !span.overlaps(entry.location.span()));
        self.definitions.push(Definition {
            location,
            value: T::retain(TrackedValue::defined(body, value)?),
            first_write: Some(self.writes),
        });
        self.writes += 1;
        Ok(())
    }

    fn dirty(&self) -> Vec<usize> {
        let mut entries: Vec<_> = self
            .definitions
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.value.is_dirty().then_some(index))
            .collect();
        entries.sort_by_key(|&index| self.definitions[index].first_write);
        entries
    }

    fn flush(
        &mut self,
        body: &mut BlockBuilder<'_>,
        span: Span,
        replacing: bool,
    ) -> Result<(), BuildError> {
        for index in self.dirty() {
            let entry = &mut self.definitions[index];
            let overlap = entry.location.span();
            if span.overlaps(overlap) && !(replacing && span.covers(overlap)) {
                entry
                    .value
                    .store(body, self.memory, &self.base, entry.location.offset)?;
                entry.value.mark_clean();
            }
        }
        Ok(())
    }

    /// Emits the current dirty definitions on a terminating path without changing
    /// the definitions used to construct other paths. This is not a backing rollback.
    pub(crate) fn publish(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        for index in self.dirty() {
            let entry = &self.definitions[index];
            entry
                .value
                .store(body, self.memory, &self.base, entry.location.offset)?;
        }
        Ok(())
    }
}
