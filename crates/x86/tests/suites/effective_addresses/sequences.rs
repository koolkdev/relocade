use crate::support::sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case};
use wasm86_x86::{FlagBytes, StoredStatusSource};
use wasm86_x86::{Gpr32::*, StoredFlags};

#[rustfmt::skip]
fn sequences() -> Vec<Case> {
    vec![Case::preserving_flags("mixed-width writes feed old base and index values")
        .instruction_count(0xffff_fffd)
        .stored_flags(StoredFlags {
            status_source: StoredStatusSource {
                kind: 9,
                reserved: [0xa5; 3],
                left: 0x7fff_fffe,
                right: 0xffff_fffe,
            },
            bytes: FlagBytes {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 1,
                of: 1,
                tf: 0,
                df: 1,
                nt: 0,
                ac: 0,
                id: 0,
                reserved: 0xa5,
            },
        })
        .initial_registers(&[(Eax, 0x1234_5678), (Ebx, 0x1111_4000), (Ecx, 0x8000_0108), (Esp, 0x9000_0100)])
        .step(Step::preserving_flags(&[0x66, 0xbb, 0xf0, 0xff]).register(Ebx, 0x1111_fff0))
        .step(Step::preserving_flags(&[0xb1, 4]).register(Ecx, 0x8000_0104))
        .step(Step::preserving_flags(&[0x8d, 0x44, 0x8b, 0x20]).register(Eax, 0x1112_0420))
        .step(Step::preserving_flags(&[0x66, 0x8d, 0x64, 0x40, 0x80]).register(Esp, 0x9000_0be0))
        .step(Step::preserving_flags(&[0x8d, 0x24, 0x24]).register(Esp, 0x9000_0be0))
        .step(Step::preserving_flags(&[0x8d, 0x4c, 0x8c, 0x10]).register(Ecx, 0x9000_1000))
        .step(Step::preserving_flags(&[0x66, 0x8d, 0x49, 0xff]).register(Ecx, 0x9000_0fff))
        .step(Step::preserving_flags(&[0x8d, 0x31]).register(Esi, 0x9000_0fff))]
}

test_sequences!(mixed_address_aliases, sequences());
