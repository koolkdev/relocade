//! Varies lazy source kinds and stale bytes to check carry reads and preserved logical flags.

use crate::support::cases::Flags;
use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{bit_at_a_time_model, image, OPERATIONS};

#[test]
fn incoming_carry_and_preserved_status_are_read_from_every_lazy_record_kind() {
    struct Source {
        name: &'static str,
        kind: u8,
        left: u32,
        right: u32,
        flags: [u8; 6],
    }
    for source in [
        Source {
            name: "byte subtraction borrows",
            kind: 1,
            left: 0x1234_5600,
            right: 0x8765_4301,
            flags: [1, 1, 1, 0, 1, 0],
        },
        Source {
            name: "word subtraction overflows without borrowing",
            kind: 5,
            left: 0x1234_8000,
            right: 0x8765_0001,
            flags: [0, 1, 1, 0, 0, 1],
        },
        Source {
            name: "dword subtraction borrows",
            kind: 9,
            left: 0,
            right: 1,
            flags: [1, 1, 1, 0, 1, 0],
        },
        Source {
            name: "byte addition carries into zero",
            kind: 2,
            left: 0x1234_56ff,
            right: 0x8765_4301,
            flags: [1, 1, 1, 1, 0, 0],
        },
        Source {
            name: "word addition overflows without carrying",
            kind: 6,
            left: 0x1234_7fff,
            right: 0x8765_0001,
            flags: [0, 1, 1, 0, 1, 1],
        },
        Source {
            name: "dword addition carries into zero",
            kind: 10,
            left: u32::MAX,
            right: 1,
            flags: [1, 1, 1, 1, 0, 0],
        },
        Source {
            name: "byte logic clears carry and has a negative result",
            kind: 3,
            left: 0x1234_5680,
            right: 0x8765_4321,
            flags: [0, 0, 0, 0, 1, 0],
        },
        Source {
            name: "word logic clears carry and has a zero result",
            kind: 7,
            left: 0x1234_0000,
            right: 0x8765_4321,
            flags: [0, 1, 0, 1, 0, 0],
        },
        Source {
            name: "dword logic clears carry and has an even-parity result",
            kind: 11,
            left: 3,
            right: 0x8765_4321,
            flags: [0, 1, 0, 0, 0, 0],
        },
    ] {
        let [cf, pf, af, zf, sf, of] = source.flags;
        let prior = Flags {
            cf,
            pf,
            af,
            zf,
            sf,
            of,
        };
        for operation in OPERATIONS {
            for (bits, input, upper, ring_count) in [
                (8, 0x81, 0x4433_2200, 9),
                (16, 0x8001, 0x4433_0000, 17),
                (32, 0x8000_0001, 0, 31),
            ] {
                for count in [0, 1, ring_count, 32] {
                    let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                    code.extend_from_slice(&[
                        0xd2 + u8::from(bits != 8),
                        0xc0 | (operation.extension() << 3),
                    ]);
                    // The stored concrete carry contradicts the lazy source.
                    let mut image = image(&code, 1 - prior.cf);
                    image.cpu.flags.status_source.kind = source.kind;
                    image.cpu.flags.status_source.left = source.left;
                    image.cpu.flags.status_source.right = source.right;
                    image.cpu.registers.eax = upper | input;
                    image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
                    let result = bit_at_a_time_model(operation, bits, input, count, prior);
                    let mut cpu = image.cpu;
                    cpu.registers.eax = upper | result.value;
                    result.apply_flags(&mut cpu);
                    cpu.eip += code.len() as u32;
                    cpu.instruction_count = 0;
                    both(
                        TestModule::interpreter(),
                        &format!("{operation:?} {bits}-bit by {count}: {}", source.name),
                        &code,
                        1,
                        &image,
                        &[Step {
                            cpu,
                            ram: &[],
                            exit: Exit::Dispatch(cpu.eip),
                        }],
                    );
                }
            }
        }
    }
}
