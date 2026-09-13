use wasm86_x86::{compile_block_from_bytes, CpuState, Registers, Segments};
use wasmparser::Validator;

use super::step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};

pub(crate) struct Image {
    pub(crate) cpu: CpuState,
    pub(crate) guest: Vec<(u32, Vec<u8>)>,
    pub(crate) machine: Vec<(u32, Vec<u8>)>,
}

impl Image {
    pub(crate) fn new(code: &[u8]) -> Self {
        let mut image = Self::empty();
        image.data(0x3000, code);
        image.map(1, 0x3000, false);
        image
    }

    pub(crate) fn empty() -> Self {
        let mut cpu = CpuState::filled(0xa5);
        cpu.registers = Registers {
            eax: 0x1111_1111,
            ecx: 0x2222_2222,
            edx: 0xdead_beef,
            ebx: 0x4444_4444,
            esp: 0x5555_5555,
            ebp: 0x6666_6666,
            esi: 0x7777_7777,
            edi: 0x8888_8888,
        };
        cpu.eip = 0x1000;
        cpu.instruction_count = u32::MAX;
        cpu.segments = Segments::flat32();
        cpu.reserved_tail.fill(0);
        Self {
            cpu,
            guest: vec![],
            machine: vec![],
        }
    }
    pub(crate) fn map(&mut self, page: u32, frame: u32, writable: bool) {
        let entry = frame | 1 | if writable { 2 } else { 0 };
        self.machine.push((page * 4, entry.to_le_bytes().to_vec()));
    }
    pub(crate) fn data(&mut self, offset: u32, bytes: &[u8]) {
        self.guest.push((offset, bytes.to_vec()));
    }
    pub(crate) fn input(&self) -> Input {
        Input {
            guest: self.guest.clone(),
            machine: self.machine.clone(),
            observe_guest: true,
            ..Input::new(&self.cpu.to_bytes())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Exit {
    Dispatch(u32),
    DivideError,
    GeneralProtection { error: u16 },
    StackFault { error: u16 },
    PageFault { address: u32, error: u16 },
    Other(u64),
}

impl Exit {
    pub(crate) fn from_word(word: u64) -> Self {
        if word == 0x0001_0000_0000_0000 {
            Self::DivideError
        } else if word >> 48 == 2 {
            Self::GeneralProtection {
                error: (word >> 32) as u16,
            }
        } else if word >> 48 == 16 {
            Self::StackFault {
                error: (word >> 32) as u16,
            }
        } else if word >> 48 == 4 {
            Self::PageFault {
                address: word as u32,
                error: (word >> 32) as u16,
            }
        } else {
            Self::Other(word)
        }
    }
}
pub(crate) struct Step<'a> {
    pub(crate) cpu: CpuState,
    pub(crate) ram: &'a [(u32, &'a [u8])],
    pub(crate) exit: Exit,
}

pub(crate) fn expected(image: &Image, steps: &[Step<'_>]) -> Observation {
    let mut initial_ram = vec![0; 65536];
    for (offset, bytes) in &image.guest {
        initial_ram[*offset as usize..*offset as usize + bytes.len()].copy_from_slice(bytes);
    }
    let mut ram = initial_ram.clone();
    let mut events = Vec::new();
    for step in steps {
        for &(offset, bytes) in step.ram {
            ram[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);
        }
        let snapshot = Snapshot {
            cpu: step.cpu.to_bytes().to_vec(),
            guest: Some(
                initial_ram
                    .iter()
                    .zip(&ram)
                    .enumerate()
                    .filter(|(_, (old, new))| old != new)
                    .map(|(offset, (_, &new))| (offset as u32, new))
                    .collect(),
            ),
        };
        let outcome = match step.exit {
            Exit::Dispatch(eip) => {
                events.push(Event::Dispatch {
                    eip: eip as i32,
                    snapshot: snapshot.clone(),
                });
                Outcome::Returned(vec![Argument::I64(i64::MIN)])
            }
            Exit::DivideError => Outcome::Returned(vec![Argument::I64(0x0001_0000_0000_0000)]),
            Exit::GeneralProtection { error } => Outcome::Returned(vec![Argument::I64(
                ((2_u64 << 48) | (u64::from(error) << 32)) as i64,
            )]),
            Exit::StackFault { error } => Outcome::Returned(vec![Argument::I64(
                ((16_u64 << 48) | (u64::from(error) << 32)) as i64,
            )]),
            Exit::PageFault { address, error } => Outcome::Returned(vec![Argument::I64(
                ((4_u64 << 48) | (u64::from(error) << 32) | u64::from(address)) as i64,
            )]),
            Exit::Other(word) => Outcome::Returned(vec![Argument::I64(word as i64)]),
        };
        events.push(Event::Return { outcome, snapshot });
    }
    Observation {
        events,
        guest_unchanged: ram == initial_ram,
        machine_unchanged: true,
    }
}

pub(crate) fn check(module: &TestModule, name: &str, image: &Image, steps: &[Step<'_>]) {
    assert_eq!(
        module.observe(&image.input(), steps.len()),
        expected(image, steps),
        "{name}, {}",
        module.entry
    );
}
pub(crate) fn both(
    step: &TestModule,
    name: &str,
    code: &[u8],
    count: u32,
    image: &Image,
    steps: &[Step<'_>],
) {
    check(step, name, image, steps);
    let start = image.cpu.eip;
    let snapshot = compile_block_from_bytes(start, code, count).unwrap();
    Validator::new().validate_all(&snapshot.bytes).unwrap();
    let ram = steps
        .iter()
        .flat_map(|step| step.ram.iter().copied())
        .collect::<Vec<_>>();
    check(
        &TestModule::new(&snapshot),
        name,
        image,
        &[Step {
            cpu: steps.last().unwrap().cpu,
            ram: &ram,
            exit: steps.last().unwrap().exit,
        }],
    );
}

pub(crate) fn byte_register_image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu.registers.eax = 0x4433_2211;
    image.cpu.registers.ecx = 0x8877_6655;
    image.cpu.registers.edx = 0xccbb_aa99;
    image.cpu.registers.ebx = 0x10ff_eedd;
    image
}
