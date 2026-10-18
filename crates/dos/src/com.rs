use crate::{memory, LoadError};
use wasm86_machine::Machine;
use wasm86_x86::{CpuState, Registers, Segment, Segments, StoredFlags, StoredSegment};

// One segment holds the PSP, COM image and stack.
const SEGMENT_SIZE: u32 = 64 * 1024;
// DOS reserves a 256-byte Program Segment Prefix before the COM image.
const PSP_SIZE: usize = 0x100;
// Reserve the final word for IP=0: a near RET reaches the PSP's INT 20h.
const STACK_OFFSET: u16 = (SEGMENT_SIZE - 2) as u16;

pub(crate) fn load(machine: &mut Machine<'_>, image: &[u8]) -> Result<(), LoadError> {
    if image.is_empty() {
        return Err(LoadError::EmptyImage);
    }
    if image.len() > usize::from(STACK_OFFSET) - PSP_SIZE {
        return Err(LoadError::ImageTooLarge);
    }
    memory::reset(&mut machine.memory());
    let psp_segment = memory::allocate(
        &mut machine.memory(),
        memory::AVAILABLE_PARAGRAPHS,
        memory::FIRST_SEGMENT,
    )
    .expect("fresh DOS arena fits COM allocation");
    let psp_address = u32::from(psp_segment) * memory::PARAGRAPH_BYTES;

    // Initialize the first segment, including the zero return word at SS:SP.
    let mut process = vec![0; SEGMENT_SIZE as usize];
    let psp = &mut process[..PSP_SIZE];
    // PSP:0000 contains INT 20h, the process termination instruction.
    psp[..2].copy_from_slice(&[0xcd, 0x20]);
    // PSP:0002 stores the first paragraph beyond the process allocation.
    let end_segment = psp_segment + memory::AVAILABLE_PARAGRAPHS;
    psp[2..4].copy_from_slice(&end_segment.to_le_bytes());
    // PSP:0016 is the parent's PSP segment. The root process is its own parent.
    psp[0x16..0x18].copy_from_slice(&psp_segment.to_le_bytes());
    // PSP:0080 is the command-tail length (zero); 0081 holds its trailing CR.
    psp[0x81] = b'\r';
    process[PSP_SIZE..PSP_SIZE + image.len()].copy_from_slice(image);
    machine
        .memory()
        .write(psp_address, &process)
        .expect("fixed COM process allocation");

    let mut segments = Segments::real_mode();
    for segment in [Segment::Cs, Segment::Ds, Segment::Es, Segment::Ss] {
        segments[segment] = StoredSegment::real_mode(segment, psp_segment);
    }
    let mut flags = StoredFlags::default();
    flags.bytes.if_ = 1; // Start with maskable interrupts enabled.
    let cpu = CpuState {
        registers: Registers {
            esp: u32::from(STACK_OFFSET),
            ..Registers::default()
        },
        eip: PSP_SIZE as u32,
        segments,
        flags,
        ..CpuState::default()
    };
    machine
        .cpu()
        .restore(&cpu)
        .expect("real-mode COM entry state");
    Ok(())
}
