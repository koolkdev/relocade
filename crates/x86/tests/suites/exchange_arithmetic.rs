use wasm86_x86::FlagBytes;
use wasm86_x86::Gpr32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi};

use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
        Permissions::{ReadOnly, ReadWrite},
    },
    machine::{byte_register_image, Image},
    sequences::{test_sequences, Checkpoint, SequenceCase},
};

#[path = "exchange_arithmetic/cmpxchg.rs"]
mod cmpxchg;
#[path = "exchange_arithmetic/decoding.rs"]
mod decoding;
#[path = "exchange_arithmetic/memory.rs"]
mod memory;
#[path = "exchange_arithmetic/xadd.rs"]
mod xadd;

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes = FlagBytes {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 1,
        of: 1,
        ..image.cpu.flags.bytes
    };
    image.cpu.flags.bytes = FlagBytes {
        tf: 0,
        df: 1,
        nt: 0,
        ac: 0,
        id: 0,
        reserved: 0xa5,
        ..image.cpu.flags.bytes
    };
    image
}

#[rustfmt::skip]
fn exchanges_and_flags() -> Vec<SequenceCase> {
    vec![SequenceCase::new("completed exchanges and consumed flags precede a failed CMPXCHG write", Flags::all(true))
        .instruction_count(0xffff_fffd)
        .initial_register(Eax, 0x4433_01ff).initial_register(Ebx, 0x4000)
        .initial_register(Ecx, 0x8877_6655).initial_register(Edx, 0xccbb_aa99)
        .initial_register(Ebp, 0x5000).initial_register(Esi, 0x7777_7ffe).initial_register(Edi, 0x4000)
        .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadOnly)
        .backing(0x7fff, &[0x5a, 0xff, 0xff, 0x5a])
        .backing(0x9fff, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a])
        .step(Checkpoint::new(&[0x0f, 0xc0, 0xe0],
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_ff00))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc2]).register(Edx, 0xccbb_aa01))
        .step(Checkpoint::new(&[0x0f, 0xb0, 0xe3],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Ebx, 0x40ff))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x94, 0xc6]).register(Edx, 0xccbb_0101))
        .step(Checkpoint::new(&[0x66, 0x0f, 0xc1, 0x17],
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Edx, 0xccbb_ffff).expect_memory(0x4000, &[0, 1]))
        .step(Checkpoint::new(&[0x66, 0x0f, 0xb1, 0xd6],
            Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_7ffe))
        .step(Checkpoint::preserving_flags(&[0x0f, 0xb1, 0x4d, 0]).fault(0x5000, 3))]
}

test_sequences!(
    exchange_results_flags_and_later_fault,
    exchanges_and_flags()
);
