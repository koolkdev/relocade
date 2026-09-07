use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, Mem, Val, I32};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Location(pub(super) u32);

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

    fn overlaps(self, location: Location) -> bool {
        let start = u64::from(location.0);
        self.start < start + 4 && start < self.end
    }
}

struct Definition {
    location: Location,
    value: Val<I32>,
    dirty: bool,
    first_write: Option<usize>,
}

/// Fixed I32 locations in one backing memory. Reads and definitions belong to one
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

    pub(super) fn read(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        location: Location,
    ) -> Result<Val<I32>, BuildError> {
        if let Some(entry) = self
            .definitions
            .iter()
            .find(|entry| entry.location == location)
        {
            return body.value(&entry.value);
        }
        self.flush(body, Span::new(location.0, 4), None)?;
        let value = body.load::<I32>(self.memory, location.0)?;
        self.definitions.push(Definition {
            location,
            value: value.clone(),
            dirty: false,
            first_write: None,
        });
        Ok(value)
    }

    pub(super) fn define(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        location: Location,
        value: impl IntoOp<I32>,
    ) -> Result<(), BuildError> {
        let value = body.value(value)?;
        if let Some(entry) = self
            .definitions
            .iter()
            .find(|entry| entry.location == location)
        {
            let previous = body.value(&entry.value)?;
            if previous.same_expression(&value) {
                return Ok(());
            }
        }
        let span = Span::new(location.0, 4);
        self.flush(body, span, Some(location))?;
        self.definitions
            .retain(|entry| entry.location == location || !span.overlaps(entry.location));
        if let Some(entry) = self
            .definitions
            .iter_mut()
            .find(|entry| entry.location == location)
        {
            if entry.first_write.is_none() {
                entry.first_write = Some(self.writes);
                self.writes += 1;
            }
            entry.value = value;
            entry.dirty = true;
        } else {
            self.definitions.push(Definition {
                location,
                value,
                dirty: true,
                first_write: Some(self.writes),
            });
            self.writes += 1;
        }
        Ok(())
    }

    /// The supplied span must cover every possible byte of the computed access.
    pub(super) fn read_at(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        span: Span,
        address: impl IntoOp<I32>,
        offset: u32,
    ) -> Result<Val<I32>, BuildError> {
        let address = body.value(address)?;
        self.flush(body, span, None)?;
        body.load_at::<I32>(self.memory, address, offset)
    }

    /// The span must cover every possible written byte. Earlier definitions are
    /// synchronized and every possibly written location is invalidated.
    pub(super) fn write_at(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        span: Span,
        address: impl IntoOp<I32>,
        offset: u32,
        value: impl IntoOp<I32>,
    ) -> Result<(), BuildError> {
        let address = body.value(address)?;
        let value = body.value(value)?;
        self.flush(body, span, None)?;
        body.store_at(self.memory, address, offset, value)?;
        self.definitions
            .retain(|entry| !span.overlaps(entry.location));
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
        except: Option<Location>,
    ) -> Result<(), BuildError> {
        for index in self.dirty() {
            let entry = &mut self.definitions[index];
            if Some(entry.location) != except && span.overlaps(entry.location) {
                body.store(self.memory, entry.location.0, &entry.value)?;
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
            body.store(self.memory, entry.location.0, &entry.value)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
