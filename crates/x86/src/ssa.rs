use std::marker::PhantomData;

use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, Mem, MemoryInt, Val, I32, I8};

#[derive(Clone, Copy)]
pub(super) struct Location<T: SsaType> {
    offset: u32,
    marker: PhantomData<T>,
}

impl<T: SsaType> Location<T> {
    pub(super) fn new(offset: u32) -> Self {
        Self {
            offset,
            marker: PhantomData,
        }
    }

    fn key(self) -> LocationKey {
        LocationKey {
            offset: self.offset,
            bytes: T::BYTES,
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
pub(super) struct Span {
    start: u64,
    end: u64,
}

impl Span {
    pub(super) fn new(offset: u32, bytes: u32) -> Self {
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

/// Values retain their logical type; byte definitions need no mask before a store.
pub(super) enum DefinitionValue {
    Byte(Val<I8>),
    Dword(Val<I32>),
}

impl DefinitionValue {
    fn store(
        &self,
        body: &mut FunctionBuilder<'_>,
        memory: Mem,
        offset: u32,
    ) -> Result<(), BuildError> {
        match self {
            Self::Byte(value) => body.store(memory, offset, value),
            Self::Dword(value) => body.store(memory, offset, value),
        }
    }
}

pub(super) trait SsaType: MemoryInt {
    fn retain(value: Val<Self>) -> DefinitionValue;
    fn value(definition: &DefinitionValue) -> &Val<Self>;
}

macro_rules! ssa_type {
    ($ty:ty, $variant:ident) => {
        impl SsaType for $ty {
            fn retain(value: Val<Self>) -> DefinitionValue {
                DefinitionValue::$variant(value)
            }

            fn value(definition: &DefinitionValue) -> &Val<Self> {
                let DefinitionValue::$variant(value) = definition else {
                    unreachable!("a location's width determines its retained value type")
                };
                value
            }
        }
    };
}

ssa_type!(I8, Byte);
ssa_type!(I32, Dword);

struct Definition {
    location: LocationKey,
    value: DefinitionValue,
    dirty: bool,
    first_write: Option<usize>,
}

/// Typed locations in one backing memory. Reads and definitions belong to one
/// straight-line build path; terminal publication can target its descendant arms.
/// Direct writes represent completed effects, not speculative changes to undo.
/// Every backing write overlapping managed locations must pass through this owner.
pub(super) struct Environment {
    memory: Mem,
    definitions: Vec<Definition>,
    writes: usize,
}

impl Environment {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            memory,
            definitions: Vec::new(),
            writes: 0,
        }
    }

    pub(super) fn read<T: SsaType>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        location: Location<T>,
    ) -> Result<Val<T>, BuildError> {
        let location = location.key();
        if let Some(entry) = self
            .definitions
            .iter()
            .find(|entry| entry.location == location)
        {
            return body.value(T::value(&entry.value));
        }
        self.flush(body, location.span(), false)?;
        self.definitions
            .retain(|entry| !location.span().overlaps(entry.location.span()));
        let value = body.load::<T>(self.memory, location.offset)?;
        self.definitions.push(Definition {
            location,
            value: T::retain(value.clone()),
            dirty: false,
            first_write: None,
        });
        Ok(value)
    }

    pub(super) fn define<T: SsaType>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        location: Location<T>,
        value: impl IntoOp<T>,
    ) -> Result<(), BuildError> {
        let value = body.value(value)?;
        let location = location.key();
        if let Some(entry) = self
            .definitions
            .iter()
            .find(|entry| entry.location == location)
        {
            let previous = body.value(T::value(&entry.value))?;
            if previous.same_expression(&value) {
                return Ok(());
            }
        }
        let span = location.span();
        // A covering definition replaces every byte of the older value. Partial
        // overlap must first preserve its remaining bytes in backing memory.
        self.flush(body, span, true)?;
        self.definitions
            .retain(|entry| entry.location == location || !span.overlaps(entry.location.span()));
        if let Some(entry) = self
            .definitions
            .iter_mut()
            .find(|entry| entry.location == location)
        {
            if entry.first_write.is_none() {
                entry.first_write = Some(self.writes);
                self.writes += 1;
            }
            entry.value = T::retain(value);
            entry.dirty = true;
        } else {
            self.definitions.push(Definition {
                location,
                value: T::retain(value),
                dirty: true,
                first_write: Some(self.writes),
            });
            self.writes += 1;
        }
        Ok(())
    }

    /// The supplied span must cover every possible byte of the computed access.
    pub(super) fn read_at<T: SsaType>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        span: Span,
        address: impl IntoOp<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        let address = body.value(address)?;
        self.flush(body, span, false)?;
        body.load_at::<T>(self.memory, address, offset)
    }

    /// The span must cover every possible written byte. Earlier definitions are
    /// synchronized and every possibly written location is invalidated.
    pub(super) fn write_at<T: SsaType>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        span: Span,
        address: impl IntoOp<I32>,
        offset: u32,
        value: impl IntoOp<T>,
    ) -> Result<(), BuildError> {
        let address = body.value(address)?;
        let value = body.value(value)?;
        self.flush(body, span, false)?;
        body.store_at(self.memory, address, offset, value)?;
        self.definitions
            .retain(|entry| !span.overlaps(entry.location.span()));
        Ok(())
    }

    fn dirty(&self) -> Vec<usize> {
        let mut entries: Vec<_> = self
            .definitions
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.dirty.then_some(index))
            .collect();
        entries.sort_by_key(|&index| self.definitions[index].first_write);
        entries
    }

    fn flush(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        span: Span,
        replacing: bool,
    ) -> Result<(), BuildError> {
        for index in self.dirty() {
            let entry = &mut self.definitions[index];
            let overlap = entry.location.span();
            if span.overlaps(overlap) && !(replacing && span.covers(overlap)) {
                entry
                    .value
                    .store(body, self.memory, entry.location.offset)?;
                entry.dirty = false;
            }
        }
        Ok(())
    }

    /// Emits the current dirty definitions on a terminating path without changing
    /// the definitions used to construct other paths. This is not a backing rollback.
    pub(super) fn publish(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        for index in self.dirty() {
            let entry = &self.definitions[index];
            entry
                .value
                .store(body, self.memory, entry.location.offset)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
