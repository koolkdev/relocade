//! One page-table entry retained across a generated loop's iterations.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32};

use super::{PageTable, PAGE_SHIFT};

/// Transport shape for compiler loop inputs. Its fields are interpreted here.
pub(crate) type PageCacheInputs = [I32; 2];

pub(crate) struct PageCache {
    index: Val<I32>,
    entry: Val<I32>,
}

impl PageCache {
    // No page-table byte offset can equal this initial key.
    pub(crate) const EMPTY: [u32; 2] = [u32::MAX, 0];

    pub(crate) fn from_inputs([index, entry]: [Val<I32>; 2]) -> Self {
        Self { index, entry }
    }

    pub(crate) fn into_inputs(self) -> [Val<I32>; 2] {
        [self.index, self.entry]
    }

    /// Mappings must remain stable for the lifetime of these loop inputs.
    /// Retaining an entry never retains guest bytes or bypasses access checks.
    pub(in crate::memory) fn lookup(
        &mut self,
        table: PageTable,
        body: &mut FunctionBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        let index = address.unsigned().shr(PAGE_SHIFT).shl(2);
        let entry = body.if_value::<I32>(
            index.eq(&self.index),
            |hit| hit.yield_(&self.entry),
            |mut miss| {
                let entry = miss.load_at::<I32>(table.entries, &index, 0)?;
                miss.yield_(entry)
            },
        )?;
        self.index = index;
        self.entry = entry.clone();
        Ok(entry)
    }
}
