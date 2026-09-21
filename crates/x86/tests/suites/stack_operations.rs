//! Stack transfers preserve flags and commit pointers only after successful access.

use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError, Gpr32::*, SegmentAttributes, StoredSegment,
};

#[path = "stack_operations/enter.rs"]
mod enter;
#[path = "stack_operations/leave.rs"]
mod leave;
#[path = "stack_operations/registers.rs"]
mod registers;

fn stack_segment(base: u32, limit: u32, big: bool) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        selector: 0x23,
        attributes: SegmentAttributes::from_bits(if big { 0x15 } else { 0x05 }),
    }
}

fn code16() -> StoredSegment {
    StoredSegment {
        attributes: SegmentAttributes::from_bits(0x07),
        ..StoredSegment::flat_code32(0x1b)
    }
}

#[rustfmt::skip]
fn registers_and_immediates() -> Vec<Case> {
    vec![
        Case::preserving_flags("PUSH opcode register stores a complete dword", &[0x53])
            .initial_register(Ebx, 0x1234_5678).register(Esp, 0x9004, 0x9000)
            .memory(0x9000, &[0xa5; 4], ReadWrite).expect_memory(0x9000, &[0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags("PUSH word group register stores only its low half", &[0x66, 0xff, 0xf3])
            .initial_register(Ebx, 0x1234_5678).register(Esp, 0x9004, 0x9002)
            .memory(0x9001, &[0xa5; 4], ReadWrite).expect_memory(0x9002, &[0x78, 0x56]),
        Case::preserving_flags("PUSH ESP captures its entry value", &[0xff, 0xf4])
            .register(Esp, 0x9004, 0x9000).memory(0x9000, &[0xa5; 4], ReadWrite).expect_memory(0x9000, &[4, 0x90, 0, 0]),
        Case::preserving_flags("PUSH SP captures its entry low half", &[0x66, 0x54])
            .register(Esp, 0x1235_0000, 0x1234_fffe).memory(0x1234_fffe, &[0xa5; 2], ReadWrite).expect_memory(0x1234_fffe, &[0, 0]),
        Case::preserving_flags("POP opcode register reads a complete dword", &[0x5b])
            .register(Ebx, 0, 0x1234_5678).register(Esp, 0x9000, 0x9004).memory(0x9000, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::preserving_flags("POP word group register preserves its upper half", &[0x66, 0x8f, 0xc3])
            .register(Ebx, 0xabcd_0000, 0xabcd_5678).register(Esp, 0x9000, 0x9002).memory(0x9000, &[0x78, 0x56], ReadOnly),
        Case::preserving_flags("POP ESP replaces the incremented stack pointer", &[0x5c])
            .register(Esp, 0x9000, 0x1234_5678).memory(0x9000, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::preserving_flags("POP SP retains the incremented upper half", &[0x66, 0x8f, 0xc4])
            .register(Esp, 0x1234_fffe, 0x1235_beef).memory(0x1234_fffe, &[0xef, 0xbe], ReadOnly),
        Case::preserving_flags("PUSH full dword immediate", &[0x68, 0, 0, 0, 0x80])
            .register(Esp, 0x9004, 0x9000).memory(0x9000, &[0xa5; 4], ReadWrite).expect_memory(0x9000, &[0, 0, 0, 0x80]),
        Case::preserving_flags("PUSH full word immediate", &[0x66, 0x68, 0xef, 0xbe])
            .register(Esp, 0x9004, 0x9002).memory(0x9002, &[0xa5; 2], ReadWrite).expect_memory(0x9002, &[0xef, 0xbe]),
        Case::preserving_flags("PUSH signed byte 7f remains positive", &[0x6a, 0x7f])
            .register(Esp, 0x9004, 0x9000).memory(0x9000, &[0xa5; 4], ReadWrite).expect_memory(0x9000, &[0x7f, 0, 0, 0]),
        Case::preserving_flags("PUSH signed byte 80 extends to a dword", &[0x6a, 0x80])
            .register(Esp, 0x9004, 0x9000).memory(0x9000, &[0xa5; 4], ReadWrite).expect_memory(0x9000, &[0x80, 0xff, 0xff, 0xff]),
        Case::preserving_flags("PUSH signed byte ff extends to a word", &[0x66, 0x6a, 0xff])
            .register(Esp, 0x9004, 0x9002).memory(0x9002, &[0xa5; 2], ReadWrite).expect_memory(0x9002, &[0xff, 0xff]),
    ]
}

#[rustfmt::skip]
fn memory_addresses() -> Vec<Case> {
    vec![
        Case::preserving_flags("PUSH memory reads the old ESP before an overlapping write", &[0xff, 0x74, 0x24, 0xfe])
            .register(Esp, 0x5004, 0x5000).memory(0x5000, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88], ReadWrite)
            .expect_memory(0x5000, &[0x33, 0x44, 0x55, 0x66]),
        Case::preserving_flags("PUSH word memory reads the old ESP", &[0x66, 0xff, 0x34, 0x24])
            .register(Esp, 0x5004, 0x5002).memory(0x5002, &[0xa5, 0xa5, 0x55, 0x66], ReadWrite).expect_memory(0x5002, &[0x55, 0x66]),
        Case::preserving_flags("POP memory uses the incremented ESP in its destination", &[0x8f, 0x04, 0x24])
            .register(Esp, 0x5000, 0x5004).memory(0x5000, &[0x11, 0x22, 0x33, 0x44, 0xa5, 0xa5, 0xa5, 0xa5], ReadWrite)
            .expect_memory(0x5004, &[0x11, 0x22, 0x33, 0x44]),
        Case::preserving_flags("POP word captures its source before an overlapping destination write", &[0x66, 0x8f, 0x44, 0x24, 0xff])
            .register(Esp, 0x5000, 0x5002).memory(0x5000, &[0x11, 0x22, 0x33], ReadWrite).expect_memory(0x5001, &[0x11, 0x22]),
        Case::preserving_flags("POP without an ESP address component preserves its absolute destination", &[0x8f, 0x04, 0xa5, 0, 0x60, 0, 0])
            .register(Esp, 0x5000, 0x5004).memory(0x5000, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
            .memory(0x6000, &[0xa5; 4], ReadWrite).expect_memory(0x6000, &[0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags("PUSH memory reads and writes complete split operands", &[0xff, 0x33])
            .initial_register(Ebx, 0x6fff).register(Esp, 0x5003, 0x4fff)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .map_page(6, 0xb000, ReadOnly).map_page(7, 0xe000, ReadOnly)
            .memory(0x4fff, &[0xa5; 4], ReadWrite).memory(0x6fff, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
            .expect_memory(0x4fff, &[0x78, 0x56, 0x34, 0x12]),
    ]
}

#[rustfmt::skip]
fn access_faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("PUSH register missing destination preserves ESP", &[0x50])
            .initial_register(Esp, 0x5004).fault(0x5000, 2),
        Case::preserving_flags("PUSH immediate cannot partially write a split stack operand", &[0x66, 0x6a, 0x80])
            .initial_register(Esp, 0x5001).memory(0x4fff, &[0xa5], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("PUSH memory source fault precedes a missing stack destination", &[0xff, 0x33])
            .initial_registers(&[(Ebx, 0x6fff), (Esp, 0x5004)]).fault(0x6fff, 0),
        Case::preserving_flags("PUSH memory rejects a read-only stack without changing ESP or bytes", &[0x66, 0xff, 0x33])
            .initial_registers(&[(Ebx, 0x6000), (Esp, 0x5002)])
            .memory(0x6000, &[0x78, 0x56], ReadOnly).memory(0x5000, &[0xa5; 2], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("POP SP incomplete source preserves both pointer halves", &[0x66, 0x5c])
            .initial_register(Esp, 0x4fff).memory(0x4fff, &[0x78], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("POP source fault precedes a missing memory destination", &[0x8f, 0x03])
            .initial_registers(&[(Esp, 0x4fff), (Ebx, 0x6000)]).fault(0x4fff, 0),
        Case::preserving_flags("POP destination fault preserves original ESP and all destination bytes", &[0x8f, 0x03])
            .initial_registers(&[(Esp, 0x9000), (Ebx, 0x4fff)])
            .memory(0x9000, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
            .memory(0x4fff, &[0xa5], ReadWrite).memory(0x5000, &[0xa5; 3], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("POP prospective ESP destination can fault without committing ESP", &[0x8f, 0x44, 0x8c, 0xf4])
            .initial_registers(&[(Esp, 0x4ffc), (Ecx, 3)]).memory(0x4ffc, &[0x78, 0x56, 0x34, 0x12], ReadOnly).fault(0x5000, 2),
    ]
}

#[rustfmt::skip]
fn transfer_sequences() -> Vec<Sequence> {
    vec![
        Sequence::preserving_flags("mixed word and dword transfers publish every completed instruction")
            .initial_registers(&[(Eax, 0x1234_5678), (Esp, 0x9004)]).map_page(8, 0x8000, ReadWrite)
            .map_page(9, 0xa000, ReadWrite).backing(0x8ffd, &[0xa5; 3]).backing(0xa000, &[0xa5; 5])
            .step(Step::preserving_flags(&[0x50]).register(Esp, 0x9000).expect_memory(0x9000, &[0x78, 0x56, 0x34, 0x12]))
            .step(Step::preserving_flags(&[0x66, 0x6a, 0x80]).register(Esp, 0x8ffe).expect_memory(0x8ffe, &[0x80, 0xff]))
            .step(Step::preserving_flags(&[0x66, 0x5a]).register(Edx, 0xdead_ff80).register(Esp, 0x9000))
            .step(Step::preserving_flags(&[0x5b]).register(Ebx, 0x1234_5678).register(Esp, 0x9004)),
        Sequence::preserving_flags("completed PUSH and POP effects survive a later POP destination fault")
            .initial_registers(&[(Eax, 0x1234_5678), (Ecx, 0x6000), (Esp, 0x5002)]).map_page(4, 0x8000, ReadWrite)
            .map_page(5, 0xa000, ReadWrite).backing(0x8ffd, &[0xa5; 3])
            .backing(0xa000, &[0xa5, 0xa5, 0xbe, 0xad, 0x11, 0x22, 0x33, 0x44, 0x5a])
            .step(Step::preserving_flags(&[0x50]).register(Esp, 0x4ffe).expect_memory(0x4ffe, &[0x78, 0x56, 0x34, 0x12]))
            .step(Step::preserving_flags(&[0x66, 0x8f, 0x04, 0x24])
                .register(Esp, 0x5000).expect_memory(0x5000, &[0x78, 0x56]))
            .step(Step::preserving_flags(&[0x5a]).register(Edx, 0xadbe_5678).register(Esp, 0x5004))
            .step(Step::preserving_flags(&[0x8f, 0x01]).fault(0x6000, 2)),
    ]
}

#[test]
fn encoding_fields() {
    for code in [
        &[0x50][..],
        &[0x58],
        &[0x66, 0x54],
        &[0x66, 0x5c],
        &[0x60],
        &[0x66, 0x61],
        &[0xc9],
        &[0x66, 0xc9],
        &[0xc8, 0xff, 0x80, 0xc9],
        &[0x66, 0xc8, 0x80, 0xff, 0x21],
        &[0x68, 0x50, 0x58, 0x68, 0x6a],
        &[0x66, 0x68, 0x8f, 0xff],
        &[0x6a, 0x80],
        &[0x66, 0x6a, 0xff],
        &[0xff, 0xf4],
        &[0x8f, 0xc4],
        &[0xff, 0x34, 0x24],
        &[0x66, 0x8f, 0x84, 0x25, 0x11, 0x22, 0x33, 0x44],
    ] {
        check_length(code);
    }
}

#[test]
fn unsupported_group_extensions_precede_address_fetch() {
    for (opcode, modrm) in (1..8)
        .map(|extension| (0x8f, (extension << 3) | 4))
        .chain([(0xff, 0x3c)])
    {
        let code = [opcode, modrm];
        assert_eq!(
            compile_block_from_bytes(0x1ffe, &code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1ffe,
                opcode
            })
        );
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ffe;
        image.data(0x3ffe, &code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "unsupported stack extension does not fetch its missing SIB",
            Exit::Other(0x0008_0000_0000_1ffe | (u64::from(opcode) << 32)),
        );
    }
}

test_cases!(register_and_immediate_forms, registers_and_immediates());
test_cases!(old_and_prospective_stack_addresses, memory_addresses());
test_cases!(fault_ordering_and_pointer_commitment, access_faults());
test_sequences!(mixed_transfers_and_faults, transfer_sequences());
