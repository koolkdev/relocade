//! Segment instructions read and load selectors and query descriptors.

use super::*;
use crate::{
    address::MemoryAddress, flags::Flag, instruction::Location, register::RegisterType, Segment,
};

instruction_families! {
    LAR {
        execute: load_access_rights;
        forms { 0x0F 0x02 => word_or_dword(modrm_reg, rm16); }
    }
    LSL {
        execute: load_limit;
        forms { 0x0F 0x03 => word_or_dword(modrm_reg, rm16); }
    }
    VERR {
        execute: verify_read;
        forms { 0x0F 0x00 /4 => word(rm); }
    }
    VERW {
        execute: verify_write;
        forms { 0x0F 0x00 /5 => word(rm); }
    }
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
    PUSH_SEGMENT {
        execute: push_selector::<_>;
        effects: [memory_write];
        forms {
            0x06 => word_or_dword(segment(Segment::Es));
            0x0E => word_or_dword(segment(Segment::Cs));
            0x16 => word_or_dword(segment(Segment::Ss));
            0x1E => word_or_dword(segment(Segment::Ds));
            0x0F 0xA0 => word_or_dword(segment(Segment::Fs));
            0x0F 0xA8 => word_or_dword(segment(Segment::Gs));
        }
    }
    POP_SEGMENT {
        execute: pop_selector::<_>;
        effects: [memory_read, segment_load];
        forms {
            0x07 => word_or_dword(segment(Segment::Es));
            0x17 => word_or_dword(segment(Segment::Ss));
            0x1F => word_or_dword(segment(Segment::Ds));
            0x0F 0xA1 => word_or_dword(segment(Segment::Fs));
            0x0F 0xA9 => word_or_dword(segment(Segment::Gs));
        }
    }
    LOAD_FAR_POINTER {
        execute: load_pointer;
        effects: [segment_load];
        forms {
            0xC4 => word_or_dword(modrm_reg, mem, segment(Segment::Es));
            0xC5 => word_or_dword(modrm_reg, mem, segment(Segment::Ds));
            0x0F 0xB2 => word_or_dword(modrm_reg, mem, segment(Segment::Ss));
            0x0F 0xB4 => word_or_dword(modrm_reg, mem, segment(Segment::Fs));
            0x0F 0xB5 => word_or_dword(modrm_reg, mem, segment(Segment::Gs));
        }
    }
}

fn load_access_rights<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<I16>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let selector = source.read(execution)?;
    let descriptor_info = execution.query_segment_descriptor(&selector)?;
    execution.write_flag(Flag::ZF, descriptor_info.visible.clone())?;
    destination.update(execution, |_, previous| {
        Ok(descriptor_info
            .visible
            .select(descriptor_info.access_rights.truncate::<T>(), previous))
    })
}

fn load_limit<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<I16>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let selector = source.read(execution)?;
    let descriptor_info = execution.query_segment_descriptor(&selector)?;
    execution.write_flag(Flag::ZF, descriptor_info.visible.clone())?;
    destination.update(execution, |_, previous| {
        Ok(descriptor_info
            .visible
            .select(descriptor_info.limit.truncate::<T>(), previous))
    })
}

fn verify_read(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: TypedLocation<I16>,
) -> Result<(), BuildError> {
    let selector = source.read(execution)?;
    let descriptor_info = execution.query_segment_descriptor(&selector)?;
    execution.write_flag(Flag::ZF, descriptor_info.readable)
}

fn verify_write(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: TypedLocation<I16>,
) -> Result<(), BuildError> {
    let selector = source.read(execution)?;
    let descriptor_info = execution.query_segment_descriptor(&selector)?;
    execution.write_flag(Flag::ZF, descriptor_info.writable)
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

fn push_selector<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    segment: Segment,
) -> Result<(), BuildError> {
    let selector = execution.read_segment_selector(segment)?;
    execution.push(selector, T::BYTES)
}

fn pop_selector<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    segment: Segment,
) -> Result<(), BuildError> {
    execution.pop_segment(segment, T::BYTES)
}

fn load_pointer<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: MemoryAddress<Val<I32>>,
    segment: Segment,
) -> Result<(), BuildError> {
    let Location::Register(register) = destination.into_location() else {
        unreachable!("the form binds a ModRM register destination")
    };
    execution.load_far_pointer(segment, register.view::<T>(), source)
}
