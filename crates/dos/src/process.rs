//! Initialization shared by the COM and MZ loaders.

use crate::{memory, LoadError, LoadOptions};
use relocade_machine::{CpuStorage, Memory};
use relocade_x86::{CpuState, FlagsState, Segment, SegmentRegister, Segments};

pub(crate) const COMMAND_TAIL_OFFSET: u16 = 0x80;
pub(crate) const PSP_BYTES: usize = 0x100;
pub(crate) const PSP_PARAGRAPHS: u16 = 0x10;

pub(crate) struct Allocation {
    pub(crate) segment: u16,
    pub(crate) paragraphs: u16,
}

/// Validates startup inputs and memory requirements before replacing the process.
pub(crate) fn allocate(
    memory: &mut Memory<'_, impl CpuStorage + ?Sized>,
    minimum: usize,
    maximum: usize,
    options: &LoadOptions<'_>,
) -> Result<Allocation, LoadError> {
    options.validate_command_tail()?;
    let environment = options.environment_block()?;
    let environment_paragraphs =
        environment.len().div_ceil(memory::PARAGRAPH_BYTES as usize) as u16;
    // Reserve the environment's data and its MCB in the same arena.
    let available = memory::AVAILABLE_PARAGRAPHS - environment_paragraphs - 1;
    if minimum > usize::from(available) {
        return Err(LoadError::InsufficientMemory);
    }
    let paragraphs = maximum.max(minimum).min(usize::from(available)) as u16;
    memory::reset(memory);
    let segment = memory::allocate(memory, paragraphs, memory::FIRST_SEGMENT)
        .expect("validated root-process allocation");
    let environment_segment = memory::allocate(memory, environment_paragraphs, segment)
        .expect("validated environment allocation");
    memory
        .write(
            u32::from(environment_segment) * memory::PARAGRAPH_BYTES,
            &environment,
        )
        .expect("process environment");
    let mut psp = [0; PSP_BYTES];
    // PSP:0000 contains INT 20h, the process termination instruction.
    psp[..2].copy_from_slice(&[0xcd, 0x20]);
    // PSP:0002 stores the first paragraph beyond the process allocation.
    psp[2..4].copy_from_slice(&(segment + paragraphs).to_le_bytes());
    // PSP:0016 is the parent's PSP segment. The root process is its own parent.
    psp[0x16..0x18].copy_from_slice(&segment.to_le_bytes());
    // PSP:002C points to the separately allocated environment block.
    psp[0x2c..0x2e].copy_from_slice(&environment_segment.to_le_bytes());
    // PSP:0080 holds the byte count; the tail and its CR follow at 0081.
    let tail_offset = usize::from(COMMAND_TAIL_OFFSET);
    psp[tail_offset] = options.command_tail.len() as u8;
    let tail_end = tail_offset + 1 + options.command_tail.len();
    psp[tail_offset + 1..tail_end].copy_from_slice(options.command_tail);
    psp[tail_end] = b'\r';
    memory
        .write(u32::from(segment) * memory::PARAGRAPH_BYTES, &psp)
        .expect("process PSP");
    Ok(Allocation {
        segment,
        paragraphs,
    })
}

pub(crate) fn initial_cpu(psp_segment: u16) -> CpuState {
    let mut segments = Segments::real_mode();
    for segment in [Segment::Cs, Segment::Ds, Segment::Es, Segment::Ss] {
        segments[segment] = SegmentRegister::real_mode(segment, psp_segment);
    }
    let mut flags = FlagsState::default();
    flags.bytes.if_ = 1; // Start with maskable interrupts enabled.
    CpuState {
        segments,
        flags,
        ..CpuState::default()
    }
}
