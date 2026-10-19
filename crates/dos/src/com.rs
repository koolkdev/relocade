use crate::{memory, process, LoadError};
use wasm86_machine::Machine;

// One segment holds the PSP, COM image and stack.
const SEGMENT_SIZE: u32 = 64 * 1024;
// Reserve the final word for IP=0: a near RET reaches the PSP's INT 20h.
const STACK_OFFSET: u16 = (SEGMENT_SIZE - 2) as u16;

pub(crate) fn load(machine: &mut Machine<'_>, image: &[u8]) -> Result<u16, LoadError> {
    if image.is_empty() {
        return Err(LoadError::EmptyImage);
    }
    if image.len() > usize::from(STACK_OFFSET) - process::PSP_BYTES {
        return Err(LoadError::ImageTooLarge);
    }
    let psp_segment = process::allocate(&mut machine.memory(), memory::AVAILABLE_PARAGRAPHS);
    let psp_address = u32::from(psp_segment) * memory::PARAGRAPH_BYTES;

    // Clear the rest of the first segment, including the zero return word at SS:SP.
    let mut contents = vec![0; SEGMENT_SIZE as usize - process::PSP_BYTES];
    contents[..image.len()].copy_from_slice(image);
    machine
        .memory()
        .write(psp_address + process::PSP_BYTES as u32, &contents)
        .expect("fixed COM process allocation");

    let mut cpu = process::initial_cpu(psp_segment);
    cpu.registers.esp = u32::from(STACK_OFFSET);
    cpu.eip = process::PSP_BYTES as u32;
    machine
        .cpu()
        .restore(&cpu)
        .expect("real-mode COM entry state");
    Ok(psp_segment)
}
