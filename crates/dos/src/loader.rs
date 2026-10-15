use crate::{services, Dos};
use std::fmt;
use wasm86_machine::{CpuContext, HostReturn, Machine};
use wasm86_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segment, Segments,
    StoredSegment,
};

const PSP: u16 = 0x1000;
const ENVIRONMENT: u16 = 0x0ff0;
const SERVICES_SEGMENT: u16 = 0xf000;
const INT20: u16 = 0x0100;
const INT21: u16 = 0x0110;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadError {
    ImageTooLarge,
    CommandTailTooLong,
}
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ImageTooLarge => {
                "COM image exceeds 65278 bytes (64 KiB minus PSP and return word)"
            }
            Self::CommandTailTooLong => "DOS command tail exceeds 126 bytes",
        })
    }
}
impl std::error::Error for LoadError {}

/// Loads a COM image at PSP:0100 in one 64-KiB allocation. The final two bytes are
/// reserved for a zero return word, so a near RET reaches the PSP's INT 20h.
/// The command tail excludes its length byte and terminating carriage return.
/// No command-line parsing, environment inheritance or DOS memory allocation is implied.
pub fn load_com(image: &[u8], command_tail: &[u8]) -> Result<Machine<Dos>, LoadError> {
    if image.len() > 0xfefe {
        return Err(LoadError::ImageTooLarge);
    }
    if command_tail.len() > 126 {
        return Err(LoadError::CommandTailTooLong);
    }
    let mut cpu = CpuState {
        eip: 0x100,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    cpu.segments.cs = StoredSegment::real_mode(Segment::Cs, PSP);
    cpu.segments.ds = StoredSegment::real_mode(Segment::Ds, PSP);
    cpu.segments.es = StoredSegment::real_mode(Segment::Es, PSP);
    cpu.segments.ss = StoredSegment::real_mode(Segment::Ss, PSP);
    cpu.registers.esp = 0xfffe;
    cpu.flags.bytes.if_ = 1;
    let context = CpuContext::new(Compiler::new(ExecutionProfile::Real16).target(), cpu)
        .expect("canonical DOS real-mode context");
    let map = PhysicalMemoryMap::new([(0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 })])
        .expect("fixed conventional RAM range is page aligned");
    let mut machine = Machine::new(context, map, Dos::default()).unwrap();
    let mut psp = [0; 256];
    psp[..2].copy_from_slice(&[0xcd, 0x20]);
    psp[2..4].copy_from_slice(&(PSP + 0x1000).to_le_bytes());
    psp[0x16..0x18].copy_from_slice(&PSP.to_le_bytes());
    psp[0x18..0x2c].fill(0xff);
    psp[0x18..0x1b].copy_from_slice(&[0, 1, 2]);
    psp[0x2c..0x2e].copy_from_slice(&ENVIRONMENT.to_le_bytes());
    psp[0x32..0x38].copy_from_slice(&[20, 0, 0x18, 0, PSP as u8, (PSP >> 8) as u8]);
    psp[0x50..0x53].copy_from_slice(&[0xcd, 0x21, 0xcb]);
    psp[0x80] = command_tail.len() as u8;
    psp[0x81..0x81 + command_tail.len()].copy_from_slice(command_tail);
    psp[0x81 + command_tail.len()] = b'\r';
    machine
        .initialize_backing(u32::from(PSP) << 4, &psp)
        .unwrap();
    machine
        .initialize_backing((u32::from(PSP) << 4) + 0x100, image)
        .unwrap();
    machine
        .initialize_backing((u32::from(PSP) << 4) + 0xfffe, &[0, 0])
        .unwrap();
    machine
        .initialize_backing(u32::from(ENVIRONMENT) << 4, b"\0\0\x01\0C:\\PROGRAM.COM\0")
        .unwrap();
    for (vector, offset) in [(0x20, INT20), (0x21, INT21)] {
        let mut pointer = [0; 4];
        pointer[..2].copy_from_slice(&offset.to_le_bytes());
        pointer[2..].copy_from_slice(&SERVICES_SEGMENT.to_le_bytes());
        machine.initialize_backing(vector * 4, &pointer).unwrap();
    }
    machine.host_functions_mut().insert(
        (u32::from(SERVICES_SEGMENT) << 4) + u32::from(INT20),
        HostReturn::Interrupt,
        services::terminate,
    );
    machine.host_functions_mut().insert(
        (u32::from(SERVICES_SEGMENT) << 4) + u32::from(INT21),
        HostReturn::Interrupt,
        services::int21,
    );
    Ok(machine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_bounds_keep_the_command_tail_and_return_word_out_of_the_image() {
        assert!(load_com(&vec![0; 0xfefe], &[b'x'; 126]).is_ok());
        assert!(matches!(
            load_com(&vec![0; 0xfeff], &[]),
            Err(LoadError::ImageTooLarge)
        ));
        assert!(matches!(
            load_com(&[], &[b'x'; 127]),
            Err(LoadError::CommandTailTooLong)
        ));
    }
}
