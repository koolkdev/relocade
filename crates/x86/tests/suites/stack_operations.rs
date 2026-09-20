use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
    RegisterExpectation::Exact,
};
use wasm86_x86::Gpr32::{self, *};

#[path = "stack_operations/decoding.rs"]
mod decoding;
#[path = "stack_operations/faults.rs"]
mod faults;
#[path = "stack_operations/leave.rs"]
mod leave;
#[path = "stack_operations/memory.rs"]
mod memory;
#[path = "stack_operations/sequences.rs"]
mod sequences;

struct Register {
    encoding: u8,
    name: Gpr32,
    input: u32,
    word_pop: u32,
}

#[rustfmt::skip]
const REGISTERS: [Register; 8] = [
    Register { encoding: 0, name: Eax, input: 0x1111_1111, word_pop: 0x1111_5678 },
    Register { encoding: 1, name: Ecx, input: 0x2222_2222, word_pop: 0x2222_5678 },
    Register { encoding: 2, name: Edx, input: 0xdead_beef, word_pop: 0xdead_5678 },
    Register { encoding: 3, name: Ebx, input: 0x4444_4444, word_pop: 0x4444_5678 },
    Register { encoding: 4, name: Esp, input: 0x0000_9004, word_pop: 0x0000_5678 },
    Register { encoding: 5, name: Ebp, input: 0x6666_6666, word_pop: 0x6666_5678 },
    Register { encoding: 6, name: Esi, input: 0x7777_7777, word_pop: 0x7777_5678 },
    Register { encoding: 7, name: Edi, input: 0x8888_8888, word_pop: 0x8888_5678 },
];

#[rustfmt::skip]
fn register_push_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for register in &REGISTERS {
        for suffix in [vec![0x50 + register.encoding], vec![0xff, 0xf0 | register.encoding]] {
            for (prefix, stack, width) in [(&[][..], 0x9000, 4), (&[0x66][..], 0x9002, 2)] {
                let code = [prefix, &suffix].concat();
                let mut case = Case::preserving_flags(format!("PUSH {:?} via {code:02x?}", register.name), &code)
                    .register(Esp, 0x9004, stack).memory(0x8fff, &[0xa5; 10], ReadWrite)
                    .expect_memory(stack, &register.input.to_le_bytes()[..width]);
                if register.name != Esp { case = case.initial_register(register.name, register.input); }
                cases.push(case);
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn register_pop_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for register in &REGISTERS {
        for suffix in [vec![0x58 + register.encoding], vec![0x8f, 0xc0 | register.encoding]] {
            for (prefix, stack, output) in [(&[][..], 0x9004, 0x9abc_5678), (&[0x66][..], 0x9002, register.word_pop)] {
                let code = [prefix, &suffix].concat();
                let mut case = Case::preserving_flags(format!("POP {:?} via {code:02x?}", register.name), &code)
                    .initial_register(Esp, 0x9000).expect_register(register.name, Exact(output))
                    .memory(0x8fff, &[0xa5, 0x78, 0x56, 0xbc, 0x9a, 0x5a], ReadOnly);
                if register.name != Esp {
                    case = case.initial_register(register.name, register.input).expect_register(Esp, Exact(stack));
                }
                cases.push(case);
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn immediate_cases() -> Vec<Case> {
    [
        (&[0x68, 0, 0, 0, 0x80][..], 0x9000, &[0, 0, 0, 0x80][..]),
        (&[0x66, 0x68, 0xef, 0xbe][..], 0x9002, &[0xef, 0xbe]),
        (&[0x6a, 0][..], 0x9000, &[0, 0, 0, 0]),
        (&[0x6a, 0x7f][..], 0x9000, &[0x7f, 0, 0, 0]),
        (&[0x6a, 0x80][..], 0x9000, &[0x80, 0xff, 0xff, 0xff]),
        (&[0x6a, 0xff][..], 0x9000, &[0xff, 0xff, 0xff, 0xff]),
        (&[0x66, 0x6a, 0][..], 0x9002, &[0, 0]),
        (&[0x66, 0x6a, 0x7f][..], 0x9002, &[0x7f, 0]),
        (&[0x66, 0x6a, 0x80][..], 0x9002, &[0x80, 0xff]),
        (&[0x66, 0x6a, 0xff][..], 0x9002, &[0xff, 0xff]),
        (&[0x66, 0x66, 0x6a, 0x80][..], 0x9002, &[0x80, 0xff]),
    ].into_iter().map(|(code, stack, bytes)| {
        Case::preserving_flags(format!("PUSH immediate {code:02x?}"), code)
            .register(Esp, 0x9004, stack).memory(0x8fff, &[0xa5; 10], ReadWrite).expect_memory(stack, bytes)
    }).collect()
}

#[rustfmt::skip]
fn pop_sp_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (stack, output) in [(0x1234_fffe, 0x1235_beef), (0x1234_ffff, 0x1235_beef), (0xffff_fffe, 0x0000_beef)] {
        for code in [&[0x66, 0x5c][..], &[0x66, 0x8f, 0xc4]] {
            cases.push(Case::preserving_flags(format!("POP SP keeps incremented upper half: ESP={stack:08x}, {code:02x?}"), code)
                .register(Esp, stack, output).memory(stack, &[0xef, 0xbe], ReadOnly));
        }
    }
    cases
}

#[rustfmt::skip]
fn wrapping_stack_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("PUSH ESP wraps the stack pointer; dword fits", &[0x54])
            .register(Esp, 0, 0xffff_fffc).memory(0xffff_fffc, &[0xa5; 4], ReadWrite).expect_memory(0xffff_fffc, &[0; 4]),
        Case::preserving_flags("PUSH SP wraps the stack pointer; word fits", &[0x66, 0x54])
            .register(Esp, 0, 0xffff_fffe).memory(0xffff_fffe, &[0xa5; 2], ReadWrite).expect_memory(0xffff_fffe, &[0; 2]),
        Case::preserving_flags("POP EAX wraps ESP after reading a complete dword", &[0x58])
            .register(Esp, 0xffff_fffc, 0).register(Eax, 0x1111_1111, 0xdead_beef).memory(0xffff_fffc, &[0xef, 0xbe, 0xad, 0xde], ReadWrite),
        Case::preserving_flags("POP AX wraps ESP after reading a complete word", &[0x66, 0x58])
            .register(Esp, 0xffff_fffe, 0).register(Eax, 0x1111_1111, 0x1111_beef).memory(0xffff_fffe, &[0xef, 0xbe], ReadWrite),
    ]
}

test_cases!(register_pushes, register_push_cases());
test_cases!(register_pops, register_pop_cases());
test_cases!(push_immediates, immediate_cases());
test_cases!(pop_sp_upper_half, pop_sp_cases());
test_cases!(stack_pointer_wrap, wrapping_stack_cases());
