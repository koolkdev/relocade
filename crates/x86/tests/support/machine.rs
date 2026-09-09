use wasm86_x86::compile_block_from_bytes;
use wasmparser::Validator;

use super::step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};

pub(crate) struct Image {
    pub(crate) cpu: [u8; 152],
    pub(crate) guest: Vec<(u32, Vec<u8>)>,
    pub(crate) machine: Vec<(u32, Vec<u8>)>,
}

impl Image {
    pub(crate) fn new(code: &[u8]) -> Self {
        let mut image = Self {
            cpu: [0xa5; 152],
            guest: vec![(0x3000, code.to_vec())],
            machine: vec![],
        };
        for (offset, value) in [
            (24, 0x1111_1111),
            (28, 0x2222_2222),
            (32, 0xdead_beef),
            (36, 0x4444_4444),
            (40, 0x5555_5555),
            (44, 0x6666_6666),
            (48, 0x7777_7777),
            (52, 0x8888_8888),
            (56, 0x1000),
            (80, 0),
            (84, 0),
            (144, 0xffff_ffff),
            (148, 0),
        ] {
            image.register(offset, value);
        }
        image.map(1, 0x3000, false);
        image
    }
    pub(crate) fn register(&mut self, offset: usize, value: u32) {
        self.cpu[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
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
            ..Input::new(&self.cpu)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Exit {
    Dispatch(u32),
    PageFault { address: u32, error: u16 },
    Other(u64),
    Trap,
}

impl Exit {
    pub(crate) fn from_word(word: u64) -> Self {
        if word >> 48 == 4 {
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
    pub(crate) cpu: &'a [(usize, u32)],
    pub(crate) ram: &'a [(u32, &'a [u8])],
    pub(crate) exit: Exit,
}

pub(crate) fn expected(image: &Image, steps: &[Step<'_>]) -> Observation {
    let mut cpu = image.cpu;
    let mut initial_ram = vec![0; 65536];
    for (offset, bytes) in &image.guest {
        initial_ram[*offset as usize..*offset as usize + bytes.len()].copy_from_slice(bytes);
    }
    let mut ram = initial_ram.clone();
    let mut events = Vec::new();
    for step in steps {
        for &(offset, value) in step.cpu {
            cpu[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for &(offset, bytes) in step.ram {
            ram[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);
        }
        let snapshot = Snapshot {
            cpu: cpu.to_vec(),
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
                Outcome::Returned(Some(Argument::I64(i64::MIN)))
            }
            Exit::PageFault { address, error } => Outcome::Returned(Some(Argument::I64(
                ((4_u64 << 48) | (u64::from(error) << 32) | u64::from(address)) as i64,
            ))),
            Exit::Other(word) => Outcome::Returned(Some(Argument::I64(word as i64))),
            Exit::Trap => Outcome::Trap,
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
    let start = u32::from_le_bytes(image.cpu[56..60].try_into().unwrap());
    let snapshot = compile_block_from_bytes(start, code, count).unwrap();
    Validator::new().validate_all(&snapshot.bytes).unwrap();
    let cpu = steps
        .iter()
        .flat_map(|step| step.cpu.iter().copied())
        .collect::<Vec<_>>();
    let ram = steps
        .iter()
        .flat_map(|step| step.ram.iter().copied())
        .collect::<Vec<_>>();
    check(
        &TestModule::new(&snapshot),
        name,
        image,
        &[Step {
            cpu: &cpu,
            ram: &ram,
            exit: steps.last().unwrap().exit,
        }],
    );
}

pub(crate) fn byte_register_image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    for (offset, value) in [
        (24, 0x4433_2211),
        (28, 0x8877_6655),
        (32, 0xccbb_aa99),
        (36, 0x10ff_eedd),
    ] {
        image.register(offset, value);
    }
    image
}
