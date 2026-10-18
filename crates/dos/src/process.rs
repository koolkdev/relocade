//! Initialization shared by the COM and MZ loaders.

use crate::memory;
use wasm86_machine::{MachineStorage, Memory};
use wasm86_x86::{CpuState, Segment, Segments, StoredFlags, StoredSegment};

pub(crate) const PSP_BYTES: usize = 0x100;
pub(crate) const PSP_PARAGRAPHS: u16 = 0x10;

/// Replaces the root process with an allocation whose size was already checked.
pub(crate) fn allocate(
    memory: &mut Memory<'_, impl MachineStorage + ?Sized>,
    paragraphs: u16,
) -> u16 {
    memory::reset(memory);
    let segment = memory::allocate(memory, paragraphs, memory::FIRST_SEGMENT)
        .expect("validated root-process allocation");
    let mut psp = [0; PSP_BYTES];
    // PSP:0000 contains INT 20h, the process termination instruction.
    psp[..2].copy_from_slice(&[0xcd, 0x20]);
    // PSP:0002 stores the first paragraph beyond the process allocation.
    psp[2..4].copy_from_slice(&(segment + paragraphs).to_le_bytes());
    // PSP:0016 is the parent's PSP segment. The root process is its own parent.
    psp[0x16..0x18].copy_from_slice(&segment.to_le_bytes());
    // PSP:0080 is the command-tail length (zero); 0081 holds its trailing CR.
    psp[0x81] = b'\r';
    memory
        .write(u32::from(segment) * memory::PARAGRAPH_BYTES, &psp)
        .expect("process PSP");
    segment
}

pub(crate) fn initial_cpu(psp_segment: u16) -> CpuState {
    let mut segments = Segments::real_mode();
    for segment in [Segment::Cs, Segment::Ds, Segment::Es, Segment::Ss] {
        segments[segment] = StoredSegment::real_mode(segment, psp_segment);
    }
    let mut flags = StoredFlags::default();
    flags.bytes.if_ = 1; // Start with maskable interrupts enabled.
    CpuState {
        segments,
        flags,
        ..CpuState::default()
    }
}
