//! A guest fixture for instruction behavior tests. ABI and page-map tests can
//! still use `machine::Image` and the lower-level execution observations directly.

#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, fmt};
use wasm86_x86::CpuState;

pub(crate) use super::machine::Exit;

use super::{
    machine::Image,
    step::{Argument, Engine, Event, Outcome, Snapshot, TestModule},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Permissions {
    ReadOnly,
    ReadWrite,
}

const CODE_START: u32 = 0x1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Page {
    frame: u32,
    permissions: Permissions,
}

#[derive(Clone, Copy)]
pub(crate) struct Mapping {
    pub(crate) page: u32,
    pub(crate) frame: u32,
    pub(crate) permissions: Permissions,
}

pub(crate) struct Machine {
    pub(crate) cpu: CpuState,
    code_start: u32,
    code: Vec<u8>,
    image: Image,
    pages: BTreeMap<u32, Page>,
    next_frame: u32,
}

impl Machine {
    /// Load code at 0x1000. Ordinary data setup cannot overwrite it.
    pub(crate) fn new(code: &[u8]) -> Self {
        Self::at(CODE_START, code)
    }

    /// Load code at an explicit guest address, including across a page boundary.
    pub(crate) fn at(address: u32, code: &[u8]) -> Self {
        Self::with_mappings(address, code, &[])
    }

    pub(crate) fn with_mappings(address: u32, code: &[u8], mappings: &[Mapping]) -> Self {
        assert!(
            !code.is_empty() && code.len() <= 0x1000,
            "the ordinary fixture holds at most 4096 bytes of code"
        );
        let mut image = Image::empty();
        image.cpu.eip = address;
        let mut pages = BTreeMap::new();
        for mapping in mappings {
            assert!(
                mapping.page < (1 << 20),
                "a guest page fits the 32-bit address space"
            );
            assert!(
                mapping.frame & 3 == 0 && mapping.frame <= 0xf000,
                "a present mapping needs valid physical backing and clear permission bits"
            );
            assert!(
                pages
                    .insert(
                        mapping.page,
                        Page {
                            frame: mapping.frame,
                            permissions: mapping.permissions
                        }
                    )
                    .is_none(),
                "duplicate guest page {:x}",
                mapping.page
            );
            image.map(
                mapping.page,
                mapping.frame,
                matches!(mapping.permissions, Permissions::ReadWrite),
            );
        }
        let mut machine = Self {
            cpu: image.cpu,
            code_start: address,
            code: code.to_vec(),
            image,
            pages,
            next_frame: 0x3000,
        };
        let mut cursor = address;
        let mut remaining = code;
        while !remaining.is_empty() {
            let count = remaining.len().min((0x1000 - (cursor & 0xfff)) as usize);
            let permissions = machine
                .pages
                .get(&(cursor >> 12))
                .map_or(Permissions::ReadOnly, |mapping| mapping.permissions);
            machine.initialize_memory(cursor, &remaining[..count], permissions);
            cursor = cursor.wrapping_add(count as u32);
            remaining = &remaining[count..];
        }
        machine
    }

    /// Map and initialize virtual data. Use explicit mappings and `backing`
    /// to describe physical aliasing or bytes outside mapped operands.
    pub(crate) fn memory(&mut self, address: u32, bytes: &[u8], permissions: Permissions) {
        let end = u64::from(address) + bytes.len() as u64;
        assert!(
            end <= (1_u64 << 32),
            "use a page-policy fixture for wrapping data"
        );
        let code_end = u64::from(self.code_start) + self.code.len() as u64;
        let overlaps = if code_end <= (1_u64 << 32) {
            u64::from(address) < code_end && end > u64::from(self.code_start)
        } else {
            end > u64::from(self.code_start) || u64::from(address) < code_end - (1_u64 << 32)
        };
        assert!(!overlaps, "ordinary data setup must not overwrite code");
        self.initialize_memory(address, bytes, permissions);
    }

    fn initialize_memory(&mut self, address: u32, bytes: &[u8], permissions: Permissions) {
        let writable = matches!(permissions, Permissions::ReadWrite);
        let mut address = address;
        let mut bytes = bytes;
        while !bytes.is_empty() {
            let page = address >> 12;
            if !self.pages.contains_key(&page) {
                while self.pages.values().any(|mapped| {
                    self.next_frame < mapped.frame + 0x1000
                        && mapped.frame < self.next_frame + 0x1000
                }) {
                    self.next_frame += 0x1000;
                }
                let frame = self.next_frame;
                assert!(frame < 0x10000, "test data exceeds the guest memory");
                self.next_frame += 0x1000;
                self.pages.insert(page, Page { frame, permissions });
            }
            let mapping = self.pages[&page];
            assert_eq!(
                mapping.permissions, permissions,
                "conflicting permissions for guest page {page:x}"
            );
            self.image.map(page, mapping.frame, writable);
            let offset = address & 0xfff;
            let count = bytes.len().min((0x1000 - offset) as usize);
            self.image.data(mapping.frame + offset, &bytes[..count]);
            address = address.wrapping_add(count as u32);
            bytes = &bytes[count..];
        }
    }

    pub(crate) fn backing(&mut self, offset: u32, bytes: &[u8]) {
        assert!(
            u64::from(offset) + bytes.len() as u64 <= 0x10000,
            "physical test data must fit its backing"
        );
        self.image.data(offset, bytes);
    }

    fn code_at_eip(&self) -> &[u8] {
        let offset = self.linear_eip().wrapping_sub(self.code_start) as usize;
        assert!(offset < self.code.len(), "EIP must select the fixture code");
        &self.code[offset..]
    }

    fn linear_eip(&self) -> u32 {
        self.cpu.segments.cs.base.wrapping_add(self.cpu.eip)
    }

    pub(crate) fn state(&self) -> State {
        let mut bytes = vec![0; 0x10000];
        for (offset, data) in &self.image.guest {
            bytes[*offset as usize..*offset as usize + data.len()].copy_from_slice(data);
        }
        State {
            cpu: self.cpu,
            memory: Memory {
                bytes,
                pages: self.pages.clone(),
            },
        }
    }

    pub(crate) fn run(&self, module: &TestModule, engine: Engine) -> Execution {
        self.run_many(module, engine, 1).pop().unwrap()
    }

    pub(crate) fn run_many(
        &self,
        module: &TestModule,
        engine: Engine,
        invocations: usize,
    ) -> Vec<Execution> {
        assert!(invocations > 0, "execution needs at least one invocation");
        let code = self.code_at_eip();
        assert_eq!(
            self.state().memory.read(self.linear_eip(), code.len()),
            code,
            "fixture setup changed the instruction bytes"
        );
        let mut input = self.image.input();
        input.cpu = self.cpu.to_bytes().to_vec();
        let observation = engine.observe(module, &input, invocations);
        let initial = self.state();
        let state = |snapshot: &Snapshot| {
            let mut state = initial.clone();
            state.cpu = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
            for &(offset, value) in snapshot.guest.as_ref().unwrap() {
                state.memory.bytes[offset as usize] = value;
            }
            state
        };
        let mut executions = Vec::new();
        let mut dispatches = Vec::new();
        for event in &observation.events {
            if matches!(
                event,
                Event::ResolveSegment { .. } | Event::SegmentPermissions { .. }
            ) {
                continue;
            }
            let Event::Return { outcome, snapshot } = event else {
                let Event::Dispatch { eip, snapshot } = event else {
                    unreachable!()
                };
                dispatches.push((*eip as u32, state(snapshot)));
                continue;
            };
            let exit = match outcome {
                Outcome::Trap => panic!(
                    "unexpected Wasm trap in {} starting at guest EIP {:#010x}",
                    module.entry, self.cpu.eip
                ),
                Outcome::Returned(values) => {
                    let [Argument::I64(value)] = values.as_slice() else {
                        panic!("an x86 entry returns i64");
                    };
                    if let Some((eip, _)) = dispatches.last() {
                        assert_eq!(*value, i64::MIN);
                        Exit::Dispatch(*eip)
                    } else {
                        Exit::from_word(*value as u64)
                    }
                }
            };
            executions.push(Execution {
                state: state(snapshot),
                exit,
                dispatches: std::mem::take(&mut dispatches),
                machine_unchanged: observation.machine_unchanged,
            });
        }
        assert!(
            dispatches.is_empty(),
            "execution ends with a return boundary"
        );
        assert_eq!(executions.len(), invocations);
        // Keep the host's full-memory invariant as well as the projected bytes.
        assert_eq!(
            observation.guest_unchanged,
            executions.last().unwrap().state.memory == initial.memory
        );
        executions
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Memory {
    bytes: Vec<u8>,
    pages: BTreeMap<u32, Page>,
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
        let page = self
            .pages
            .get(&(address >> 12))
            .expect("the fixture maps this virtual address");
        (page.frame + (address & 0xfff)) as usize
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
    pub(crate) cpu: CpuState,
    pub(crate) memory: Memory,
}

pub(crate) struct Execution {
    pub(crate) state: State,
    pub(crate) exit: Exit,
    pub(crate) dispatches: Vec<(u32, State)>,
    pub(crate) machine_unchanged: bool,
}
