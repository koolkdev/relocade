use crate::{memory, process, LoadError, LoadOptions};
use relocade_machine::CpuContext;

// One segment holds the PSP, COM image and stack.
const SEGMENT_SIZE: u32 = 64 * 1024;
// Reserve the final word for IP=0: a near RET reaches the PSP's INT 20h.
const STACK_OFFSET: u16 = (SEGMENT_SIZE - 2) as u16;

pub(crate) fn load<T>(
    machine: &mut CpuContext<'_, T>,
    image: &[u8],
    options: &LoadOptions<'_>,
) -> Result<u16, LoadError> {
    if image.is_empty() {
        return Err(LoadError::EmptyImage);
    }
    if image.len() > usize::from(STACK_OFFSET) - process::PSP_BYTES {
        return Err(LoadError::ImageTooLarge);
    }
    let allocation = process::allocate(
        &mut machine.memory,
        (SEGMENT_SIZE / memory::PARAGRAPH_BYTES) as usize,
        usize::from(memory::AVAILABLE_PARAGRAPHS),
        options,
    )?;
    let psp_segment = allocation.segment;
    let psp_address = u32::from(psp_segment) * memory::PARAGRAPH_BYTES;

    machine
        .memory
        .write(psp_address + process::PSP_BYTES as u32, image)
        .expect("fixed COM process allocation");
    // Earlier programs may have used this stack. RET must reach PSP:0000.
    machine
        .memory
        .write(psp_address + u32::from(STACK_OFFSET), &[0, 0])
        .expect("COM initial return word");

    let mut cpu = process::initial_cpu(psp_segment);
    cpu.registers.esp = u32::from(STACK_OFFSET);
    cpu.eip = process::PSP_BYTES as u32;
    machine
        .cpu
        .restore(&cpu)
        .expect("COM entry state satisfies the real-mode execution target");
    Ok(psp_segment)
}
