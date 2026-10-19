use relocade_x86::{page_table, CpuState, Exception, ExecutionExit, Registers, Segments};

use super::step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};

pub(crate) struct Image {
    pub(crate) cpu: CpuState,
    pub(crate) guest: Vec<(u32, Vec<u8>)>,
    pub(crate) machine: Vec<(u32, Vec<u8>)>,
    pub(crate) physical_pages: Vec<(u32, u32, bool)>,
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
        cpu.instruction_count = u64::MAX;
        cpu.segments = Segments::flat32();
        Self {
            cpu,
            guest: vec![],
            machine: vec![],
            physical_pages: vec![],
        }
    }
    pub(crate) fn map(&mut self, page: u32, frame: u32, writable: bool) {
        let offset = page_table::entry_offset(page * page_table::PAGE_BYTES);
        let entry = page_table::encode_entry(frame, writable).expect("page-aligned backing");
        self.machine.push((offset, entry.to_vec()));
        self.physical_pages.push((page, frame, writable));
    }
    pub(crate) fn data(&mut self, offset: u32, bytes: &[u8]) {
        self.guest.push((offset, bytes.to_vec()));
    }
    pub(crate) fn input(&self) -> Input {
        Input {
            guest: self.guest.clone(),
            machine: self.machine.clone(),
            physical_pages: self.physical_pages.clone(),
            observe_guest: true,
            ..Input::new(&self.cpu.to_bytes())
        }
    }

    /// Expect one exit with the entire CPU record and both memories unchanged.
    #[track_caller]
    pub(crate) fn check_unchanged_exit(&self, module: &TestModule, name: &str, exit: Exit) {
        assert_eq!(
            module.observe(&self.input(), 1),
            expected(
                self,
                &[Step {
                    cpu: self.cpu,
                    ram: &[],
                    exit
                }]
            ),
            "{name}, {}",
            module.entry,
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Exit {
    Interpret,
    Dispatch(u32),
    DivideError,
    BoundRangeExceeded,
    InvalidOpcode,
    FloatingPoint,
    SimdFloatingPoint,
    GeneralProtection { error: u16 },
    StackFault { error: u16 },
    PageFault { address: u32, error: u16 },
    Other(u64),
}

impl Exit {
    pub(crate) fn from_word(word: u64) -> Self {
        match ExecutionExit::from_bits(word) {
            Some(ExecutionExit::Exception(exception)) => match exception {
                Exception::DivideError => Self::DivideError,
                Exception::BoundRangeExceeded => Self::BoundRangeExceeded,
                Exception::InvalidOpcode => Self::InvalidOpcode,
                Exception::FloatingPoint => Self::FloatingPoint,
                Exception::SimdFloatingPoint => Self::SimdFloatingPoint,
                Exception::GeneralProtection { error_code } => Self::GeneralProtection {
                    error: error_code as u16,
                },
                Exception::StackFault { error_code } => Self::StackFault {
                    error: error_code as u16,
                },
                Exception::PageFault {
                    linear_address,
                    error_code,
                } => Self::PageFault {
                    address: linear_address,
                    error: error_code as u16,
                },
                Exception::SegmentNotPresent { .. } => Self::Other(word),
            },
            _ => Self::Other(word),
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
            Exit::Interpret => {
                events.push(Event::Interpret {
                    snapshot: snapshot.clone(),
                });
                Outcome::Returned(vec![Argument::I64(i64::MIN)])
            }
            Exit::Dispatch(eip) => {
                assert_eq!(step.cpu.eip, eip);
                events.push(Event::Dispatch {
                    snapshot: snapshot.clone(),
                });
                Outcome::Returned(vec![Argument::I64(i64::MIN)])
            }
            Exit::DivideError => Outcome::Returned(vec![Argument::I64(0x0001_0000_0000_0000)]),
            Exit::BoundRangeExceeded => {
                Outcome::Returned(vec![Argument::I64(0x0040_0000_0000_0000)])
            }
            Exit::InvalidOpcode => Outcome::Returned(vec![Argument::I64(0x0080_0000_0000_0000)]),
            Exit::FloatingPoint => Outcome::Returned(vec![Argument::I64(0x0100_0000_0000_0000)]),
            Exit::SimdFloatingPoint => {
                Outcome::Returned(vec![Argument::I64(0x0200_0000_0000_0000)])
            }
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
