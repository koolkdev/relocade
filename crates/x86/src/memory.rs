use wasm86_compiler::{
    BuildError, FunctionBuilder, Mem, MemoryImport, MemoryInt, Program, Val, I1, I32,
};

const PAGE_SHIFT: u32 = 12;
const PAGE_BYTES: u32 = 1 << PAGE_SHIFT;
const PAGE_OFFSET: u32 = PAGE_BYTES - 1;
const FRAME_MASK: u32 = !PAGE_OFFSET;
const PRESENT: u32 = 1;

#[derive(Clone, Copy)]
pub(super) struct Memory {
    guest: Mem,
    machine: Mem,
}

pub(super) struct Translation {
    pub(super) missing: Val<I1>,
    pub(super) physical: Val<I32>,
}

pub(super) struct DirectRange {
    pub(super) unavailable: Val<I1>,
    pub(super) physical: Val<I32>,
}

impl Memory {
    pub(super) fn declare(program: &mut Program) -> Self {
        let guest = program.import_memory(MemoryImport {
            module: "wasm86".into(),
            name: "guest".into(),
            minimum: 1,
            maximum: None,
        });
        let machine = program.import_memory(MemoryImport {
            module: "wasm86".into(),
            name: "machine".into(),
            minimum: 64,
            maximum: None,
        });
        Self { guest, machine }
    }

    fn entry(
        self,
        body: &mut FunctionBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        let index = address.unsigned().shr(PAGE_SHIFT).shl(2);
        body.load_at::<I32>(self.machine, index, 0)
    }

    pub(super) fn translate(
        self,
        body: &mut FunctionBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<Translation, BuildError> {
        let entry = self.entry(body, address)?;
        Ok(Translation {
            missing: entry.and(PRESENT).eq(0),
            physical: physical(&entry, address),
        })
    }

    /// Checks a positive fixed span of at most 4096 bytes for presence and
    /// physically contiguous backing.
    /// Failure only selects a checked path; it does not identify an architectural fault.
    pub(super) fn direct(
        self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        bytes: u32,
    ) -> Result<DirectRange, BuildError> {
        let first = self.entry(body, start)?;
        let crosses = start.and(PAGE_OFFSET).unsigned().ge(PAGE_BYTES - bytes + 1);
        let unavailable = body.if_value::<I1>(
            crosses,
            |mut arm| {
                let last = start.add(bytes - 1);
                let second = self.entry(&mut arm, &last)?;
                let first_frame = first.and(FRAME_MASK);
                let expected_frame = first_frame.add(PAGE_BYTES);
                let denied = first.and(&second).and(PRESENT).eq(0);
                let linear_wrap = last.unsigned().lt(start);
                let physical_wrap = expected_frame.unsigned().lt(&first_frame);
                let scattered = second.and(FRAME_MASK).ne(&expected_frame);
                arm.yield_(denied.or(linear_wrap).or(physical_wrap).or(scattered))
            },
            |arm| arm.yield_(first.and(PRESENT).eq(0)),
        )?;
        Ok(DirectRange {
            unavailable,
            physical: physical(&first, start),
        })
    }

    /// The caller must prove this entire read is present and physically contiguous.
    pub(super) fn load<T: MemoryInt>(
        self,
        body: &mut FunctionBuilder<'_>,
        physical: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        body.load_at::<T>(self.guest, physical, offset)
    }
}

fn physical(entry: &Val<I32>, address: &Val<I32>) -> Val<I32> {
    entry.and(FRAME_MASK).or(address.and(PAGE_OFFSET))
}
