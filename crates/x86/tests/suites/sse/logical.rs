//! Logical opcodes preserve raw bits, flags and MXCSR in both decoders.

use super::*;

const AND: u128 = 0x00200507_01aa8845_32108888_00543200;
const AND_NOT: u128 = 0xf0d00a08_54002210_01234444_00abcd00;
const OR: u128 = 0xf1f34f6f_ddabefff_fffffedc_76ffff10;
const XOR: u128 = 0xf1d34a68_dc0167ba_cdef7654_76abcd10;

// Prefix, opcode, result for LEFT/RIGHT, result when both operands are LEFT.
const FORMS: &[(&[u8], u8, u128, u128)] = &[
    (&[], 0x54, AND, LEFT),
    (&[0x66], 0x54, AND, LEFT),
    (&[], 0x55, AND_NOT, 0),
    (&[0x66], 0x55, AND_NOT, 0),
    (&[], 0x56, OR, LEFT),
    (&[0x66], 0x56, OR, LEFT),
    (&[], 0x57, XOR, 0),
    (&[0x66], 0x57, XOR, 0),
    (&[0x66], 0xdb, AND, LEFT),
    (&[0x66], 0xdf, AND_NOT, 0),
    (&[0x66], 0xeb, OR, LEFT),
    (&[0x66], 0xef, XOR, 0),
];

fn registers(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for &(prefix, opcode, result, self_result) in FORMS {
        for (source, result) in [(1, result), (6, self_result)] {
            let code = encoding(prefix, opcode, 0xf0 | source); // xmm6, xmm1 or xmm6
            let mut image = image(&code);
            image.cpu.simd.xmm[6] = LEFT.to_le_bytes();
            image.cpu.simd.xmm[1] = RIGHT.to_le_bytes();
            let mut cpu = retire(image.cpu, code.len());
            cpu.simd.xmm[6] = result.to_le_bytes();
            checks.check(
                &format!("logical register form {code:02x?}, including aliasing"),
                &code,
                &image,
                &[dispatch(cpu)],
            );
        }
    }
}

fn memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for &(prefix, opcode, result, _) in FORMS {
        let code = encoding(prefix, opcode, 0x30); // xmm6, [eax]
        let mut image = image(&code);
        image.cpu.simd.xmm[6] = LEFT.to_le_bytes();
        image.cpu.registers.eax = 0x4ff0;
        image.map(4, 0x8000, false);
        image.data(0x8ff0, &RIGHT.to_le_bytes());
        let mut cpu = retire(image.cpu, code.len());
        cpu.simd.xmm[6] = result.to_le_bytes();
        checks.check(
            &format!("logical memory form {code:02x?} reads exactly 16 bytes"),
            &code,
            &image,
            &[dispatch(cpu)],
        );
        image.cpu.registers.eax = 0x5001; // Misaligned and unmapped: #GP precedes #PF.
        checks.check(
            &format!("logical memory form {code:02x?} requires 16-byte alignment"),
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::GeneralProtection { error: 0 },
            }],
        );
    }
}

fn fault_after_progress(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = [0x66, 0x0f, 0xef, 0xff, 0x66, 0x0f, 0xdf, 0x30]; // pxor; pandn [eax]
    let mut image = image(&code);
    image.cpu.registers.eax = 0x5000;
    let mut cpu = retire(image.cpu, 4);
    cpu.simd.xmm[7] = [0; 16];
    checks.check(
        "logical source fault preserves its destination and earlier work",
        &code,
        &image,
        &[
            dispatch(cpu),
            Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            },
        ],
    );
}

test_frontends!(sse_logical_registers, registers);
test_frontends!(sse_logical_memory, memory);
test_frontends!(sse_logical_fault_after_progress, fault_after_progress);
