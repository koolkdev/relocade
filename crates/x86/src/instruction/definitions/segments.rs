//! Segment moves distinguish visible selectors from checked cache loads.

use super::*;
use crate::{instruction::Location, register::RegisterType, Segment};

instruction_families! {
    MOV_FROM_SEGMENT {
        execute: store_selector;
        forms {
            0x8C /0 => word_or_dword(rm, segment(Segment::Es));
            0x8C /1 => word_or_dword(rm, segment(Segment::Cs));
            0x8C /2 => word_or_dword(rm, segment(Segment::Ss));
            0x8C /3 => word_or_dword(rm, segment(Segment::Ds));
            0x8C /4 => word_or_dword(rm, segment(Segment::Fs));
            0x8C /5 => word_or_dword(rm, segment(Segment::Gs));
        }
    }
    MOV_TO_SEGMENT {
        execute: load_selector;
        effects: [segment_load];
        forms {
            0x8E /0 => word(segment(Segment::Es), rm);
            0x8E /2 => word(segment(Segment::Ss), rm);
            0x8E /3 => word(segment(Segment::Ds), rm);
            0x8E /4 => word(segment(Segment::Fs), rm);
            0x8E /5 => word(segment(Segment::Gs), rm);
        }
    }
}

fn store_selector<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    segment: Segment,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let selector = execution.read_segment_selector(segment)?;
    match destination.into_location() {
        Location::Memory(address) => execution.write::<I16>(Location::Memory(address), selector),
        register => execution.write::<T>(
            register,
            selector.unsigned().extend::<I32>().truncate::<T>(),
        ),
    }
}

fn load_selector(
    execution: &mut ExecutionBuilder<'_, '_>,
    segment: Segment,
    source: TypedLocation<I16>,
) -> Result<(), BuildError> {
    let selector = source.read(execution)?;
    execution.load_segment(segment, selector)
}
