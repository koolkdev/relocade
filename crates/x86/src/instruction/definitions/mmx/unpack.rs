//! MMX unpacking interleaves one 32-bit half from each input.

use super::*;

instruction_families! {
    PUNPCKLBW {
        execute: unpack::<_>(1, 0);
        forms { NP 0x0f 0x60 => dword(mm, mm_rm); }
    }
    PUNPCKLWD {
        execute: unpack::<_>(2, 0);
        forms { NP 0x0f 0x61 => dword(mm, mm_rm); }
    }
    PUNPCKLDQ {
        execute: unpack::<_>(4, 0);
        forms { NP 0x0f 0x62 => dword(mm, mm_rm); }
    }
    PUNPCKHBW {
        execute: unpack::<_>(1, 4);
        forms { NP 0x0f 0x68 => qword(mm, mm_rm); }
    }
    PUNPCKHWD {
        execute: unpack::<_>(2, 4);
        forms { NP 0x0f 0x69 => qword(mm, mm_rm); }
    }
    PUNPCKHDQ {
        execute: unpack::<_>(4, 4);
        forms { NP 0x0f 0x6a => qword(mm, mm_rm); }
    }
}

fn unpack<T: MemoryInt + TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: MmxLocation,
    lane_bytes: u8,
    first_byte: u8,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let lanes = std::array::from_fn(|byte| {
        let byte = byte as u8;
        let input = (byte / lane_bytes % 2) * 16;
        first_byte + input + (byte / (2 * lane_bytes)) * lane_bytes + byte % lane_bytes
    });
    // Low forms read exactly four memory bytes; high forms read eight.
    packed_binary::<T>(execution, destination, source, |left, right| {
        left.shuffle_bytes(right, lanes)
    })
}
