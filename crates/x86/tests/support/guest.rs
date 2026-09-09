//! A guest fixture for instruction behavior tests. ABI and page-map tests can
//! still use `machine::Image` and the lower-level execution observations directly.

use std::{collections::BTreeMap, fmt};

pub(crate) use super::machine::Exit;

use super::{
    machine::Image,
    step::{Argument, Event, Outcome, Snapshot, TestModule},
};

#[derive(Clone, Copy)]
pub(crate) enum Register {
    Eax = 24,
    Ecx = 28,
    Edx = 32,
    Ebx = 36,
    Esp = 40,
    Ebp = 44,
    Esi = 48,
    Edi = 52,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Cpu([u8; 152]);

impl fmt::Debug for Cpu {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Cpu")
            .field(
                "eax",
                &format_args!("{:#010x}", self.register(Register::Eax)),
            )
            .field(
                "ecx",
                &format_args!("{:#010x}", self.register(Register::Ecx)),
            )
            .field(
                "edx",
                &format_args!("{:#010x}", self.register(Register::Edx)),
            )
            .field(
                "ebx",
                &format_args!("{:#010x}", self.register(Register::Ebx)),
            )
            .field(
                "esp",
                &format_args!("{:#010x}", self.register(Register::Esp)),
            )
            .field(
                "ebp",
                &format_args!("{:#010x}", self.register(Register::Ebp)),
            )
            .field(
                "esi",
                &format_args!("{:#010x}", self.register(Register::Esi)),
            )
            .field(
                "edi",
                &format_args!("{:#010x}", self.register(Register::Edi)),
            )
            .field("eip", &format_args!("{:#010x}", self.eip()))
            .field("instruction_count", &self.instruction_count())
            .field("bytes", &format_args!("{:02x?}", self.0))
            .finish()
    }
}

impl Cpu {
    pub(crate) fn from_bytes(bytes: [u8; 152]) -> Self {
        Self(bytes)
    }

    pub(crate) fn register(&self, register: Register) -> u32 {
        self.word(register as usize)
    }

    pub(crate) fn set_register(&mut self, register: Register, value: u32) {
        self.set_word(register as usize, value);
    }

    pub(crate) fn eip(&self) -> u32 {
        self.word(56)
    }

    pub(crate) fn set_eip(&mut self, value: u32) {
        self.set_word(56, value);
    }

    pub(crate) fn instruction_count(&self) -> u32 {
        self.word(144)
    }

    pub(crate) fn set_instruction_count(&mut self, value: u32) {
        self.set_word(144, value);
    }

    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.0[offset..offset + 4].try_into().unwrap())
    }

    fn set_word(&mut self, offset: usize, value: u32) {
        self.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Permissions {
    ReadOnly,
    ReadWrite,
}

const CODE_START: u32 = 0x1000;

pub(crate) struct Machine {
    pub(crate) cpu: Cpu,
    code: Vec<u8>,
    image: Image,
    pages: BTreeMap<u32, u32>,
    next_frame: u32,
}

impl Machine {
    /// Load code at 0x1000. Ordinary data setup cannot overwrite it.
    pub(crate) fn new(code: &[u8]) -> Self {
        assert!(
            !code.is_empty() && code.len() <= 0x1000,
            "the ordinary fixture holds one page of code"
        );
        let image = Image::new(code);
        Self {
            cpu: Cpu(image.cpu),
            code: code.to_vec(),
            image,
            pages: BTreeMap::from([(1, 0x3000)]),
            next_frame: 0x4000,
        }
    }

    /// Map and initialize ordinary virtual data. Tests of physical aliasing or
    /// page-map encoding should use `Image::map` and `Image::data` instead.
    pub(crate) fn memory(&mut self, address: u32, bytes: &[u8], permissions: Permissions) {
        let end = u64::from(address) + bytes.len() as u64;
        assert!(
            end <= (1_u64 << 32),
            "use a page-policy fixture for wrapping data"
        );
        assert!(
            end <= u64::from(CODE_START)
                || u64::from(address) >= u64::from(CODE_START) + self.code.len() as u64,
            "ordinary data setup must not overwrite code"
        );
        let writable = matches!(permissions, Permissions::ReadWrite);
        let mut address = address;
        let mut bytes = bytes;
        while !bytes.is_empty() {
            let page = address >> 12;
            let frame = *self.pages.entry(page).or_insert_with(|| {
                let frame = self.next_frame;
                assert!(frame < 0x10000, "test data exceeds the guest memory");
                self.next_frame += 0x1000;
                frame
            });
            self.image.map(page, frame, writable);
            let offset = address & 0xfff;
            let count = bytes.len().min((0x1000 - offset) as usize);
            self.image.data(frame + offset, &bytes[..count]);
            address = address.wrapping_add(count as u32);
            bytes = &bytes[count..];
        }
    }

    pub(crate) fn run_step(&self) -> Execution {
        self.run(TestModule::interpreter())
    }

    pub(crate) fn run_block(&self, instruction_limit: u32) -> Execution {
        let module = wasm86_x86::compile_block_from_bytes(
            self.cpu.eip(),
            self.code_at_eip(),
            instruction_limit,
        )
        .unwrap();
        self.run(&TestModule::new(&module))
    }

    fn code_at_eip(&self) -> &[u8] {
        let offset = self.cpu.eip().wrapping_sub(CODE_START) as usize;
        assert!(offset < self.code.len(), "EIP must select the fixture code");
        &self.code[offset..]
    }

    pub(crate) fn state(&self) -> State {
        let mut bytes = vec![0; 0x10000];
        for (offset, data) in &self.image.guest {
            bytes[*offset as usize..*offset as usize + data.len()].copy_from_slice(data);
        }
        State {
            cpu: self.cpu.clone(),
            memory: Memory {
                bytes,
                pages: self.pages.clone(),
            },
        }
    }

    fn run(&self, module: &TestModule) -> Execution {
        self.code_at_eip();
        let mut input = self.image.input();
        input.cpu = self.cpu.0.to_vec();
        let observation = module.observe(&input, 1);
        let initial = self.state();
        let state = |snapshot: &Snapshot| {
            let mut state = initial.clone();
            state.cpu = Cpu(snapshot.cpu.as_slice().try_into().unwrap());
            for &(offset, value) in snapshot.guest.as_ref().unwrap() {
                state.memory.bytes[offset as usize] = value;
            }
            state
        };
        let dispatches = observation
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Dispatch { eip, snapshot } => Some((*eip as u32, state(snapshot))),
                Event::Return { .. } => None,
            })
            .collect::<Vec<_>>();
        let Event::Return { outcome, snapshot } = observation.events.last().unwrap() else {
            panic!("execution ends with a return boundary");
        };
        let exit = match outcome {
            Outcome::Trap => Exit::Trap,
            Outcome::Returned(Some(Argument::I64(value))) => {
                if let Some((eip, _)) = dispatches.last() {
                    assert_eq!(*value, i64::MIN);
                    Exit::Dispatch(*eip)
                } else {
                    Exit::from_word(*value as u64)
                }
            }
            _ => panic!("an x86 entry returns i64"),
        };
        let state = state(snapshot);
        // Keep the host's full-memory invariant as well as the projected bytes.
        assert_eq!(observation.guest_unchanged, state.memory == initial.memory);
        Execution {
            state,
            exit,
            dispatches,
            machine_unchanged: observation.machine_unchanged,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Memory {
    bytes: Vec<u8>,
    pages: BTreeMap<u32, u32>,
}

impl Memory {
    pub(crate) fn read(&self, address: u32, length: usize) -> Vec<u8> {
        (0..length)
            .map(|offset| self.bytes[self.physical(address.wrapping_add(offset as u32))])
            .collect()
    }

    /// Change an expected memory image using guest virtual addresses.
    pub(crate) fn write(&mut self, address: u32, bytes: &[u8]) {
        for (offset, &value) in bytes.iter().enumerate() {
            let physical = self.physical(address.wrapping_add(offset as u32));
            self.bytes[physical] = value;
        }
    }

    fn physical(&self, address: u32) -> usize {
        let frame = self
            .pages
            .get(&(address >> 12))
            .expect("the fixture maps this virtual address");
        (frame + (address & 0xfff)) as usize
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Zero-filled RAM need not drown out the code and data in a failure.
        let nonzero = self
            .bytes
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte != 0)
            .collect::<Vec<_>>();
        formatter
            .debug_struct("Memory")
            .field("pages", &self.pages)
            .field("nonzero_bytes", &nonzero)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct State {
    pub(crate) cpu: Cpu,
    pub(crate) memory: Memory,
}

pub(crate) struct Execution {
    pub(crate) state: State,
    pub(crate) exit: Exit,
    pub(crate) dispatches: Vec<(u32, State)>,
    pub(crate) machine_unchanged: bool,
}
