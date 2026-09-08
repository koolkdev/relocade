use std::fmt::Write as _;
use wasm86_x86::compile_block_from_bytes;
use wasmparser::Validator;

use super::step::ModuleFile;

pub(super) struct Image {
    pub(super) cpu: [u8; 152],
    pub(super) guest: Vec<(u32, Vec<u8>)>,
    pub(super) machine: Vec<(u32, Vec<u8>)>,
}

impl Image {
    pub(super) fn new(code: &[u8]) -> Self {
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
    pub(super) fn register(&mut self, offset: usize, value: u32) {
        self.cpu[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    pub(super) fn map(&mut self, page: u32, frame: u32, writable: bool) {
        let entry = frame | 1 | if writable { 2 } else { 0 };
        self.machine.push((page * 4, entry.to_le_bytes().to_vec()));
    }
    pub(super) fn data(&mut self, offset: u32, bytes: &[u8]) {
        self.guest.push((offset, bytes.to_vec()));
    }
    fn input(&self) -> String {
        let patches = |items: &[(u32, Vec<u8>)]| {
            items
                .iter()
                .map(|(offset, bytes)| format!("[{offset},{bytes:?}]"))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "[{:?},[{}],[{}],[],true]",
            self.cpu,
            patches(&self.guest),
            patches(&self.machine)
        )
    }
}

#[derive(Clone, Copy)]
pub(super) enum Exit {
    Dispatch(u32),
    Fault(u64),
    Trap,
}
pub(super) struct Step<'a> {
    pub(super) cpu: &'a [(usize, u32)],
    pub(super) ram: &'a [(u32, &'a [u8])],
    pub(super) exit: Exit,
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in bytes {
        write!(&mut text, "{byte:02x}").unwrap();
    }
    text
}
fn changes(before: &[u8], after: &[u8]) -> String {
    let entries = before
        .iter()
        .zip(after)
        .enumerate()
        .filter(|(_, (old, new))| old != new)
        .map(|(offset, (_, new))| format!("[{offset},{new}]"))
        .collect::<Vec<_>>();
    format!("[{}]", entries.join(","))
}
pub(super) fn check(
    module: &ModuleFile,
    flags: &[&str],
    name: &str,
    image: &Image,
    steps: &[Step<'_>],
) {
    let mut cpu = image.cpu;
    let mut initial_ram = vec![0; 65536];
    for (offset, bytes) in &image.guest {
        initial_ram[*offset as usize..*offset as usize + bytes.len()].copy_from_slice(bytes);
    }
    let mut ram = initial_ram.clone();
    let mut expected = String::new();
    for step in steps {
        for &(offset, value) in step.cpu {
            cpu[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for &(offset, bytes) in step.ram {
            ram[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);
        }
        let state = hex(&cpu);
        let changed = changes(&initial_ram, &ram);
        match step.exit {
            Exit::Dispatch(eip) => {
                writeln!(&mut expected, "dispatch({}) {state}", eip as i32).unwrap();
                writeln!(&mut expected, "guest at dispatch {changed}").unwrap();
                expected.push_str("return -9223372036854775808\n");
            }
            Exit::Fault(word) => writeln!(&mut expected, "return {word}").unwrap(),
            Exit::Trap => expected.push_str("return trap\n"),
        }
        writeln!(&mut expected, "state {state}\nguest at return {changed}").unwrap();
    }
    writeln!(
        &mut expected,
        "guest {}\nmachine unchanged",
        if ram == initial_ram {
            "unchanged"
        } else {
            "changed"
        }
    )
    .unwrap();
    assert_eq!(
        module.observe(flags, &image.input(), steps.len()),
        expected,
        "{name}, {}, {flags:?}",
        module.entry
    );
}
pub(super) fn both(
    step: &ModuleFile,
    flags: &[&str],
    name: &str,
    code: &[u8],
    count: u32,
    image: &Image,
    steps: &[Step<'_>],
) {
    check(step, flags, name, image, steps);
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
        &ModuleFile::new(&snapshot),
        flags,
        name,
        image,
        &[Step {
            cpu: &cpu,
            ram: &ram,
            exit: steps.last().unwrap().exit,
        }],
    );
}
