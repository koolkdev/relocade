use super::*;
use crate::support::machine::Exit;
use crate::{DescriptorTable, DescriptorTableRegister};

fn protected(engine: Engine) {
    let profile = ExecutionProfile::Protected(SegmentProfile::Flat32);
    for (code, bytes) in [
        (&[0x0f, 0x01, 0x0b][..], [0xff, 7, 0xde, 0xc0, 0xad, 0xde]),
        (
            &[0x66, 0x0f, 0x01, 0x0b][..],
            [0xff, 7, 0xde, 0xc0, 0xad, 0],
        ),
    ] {
        let mut image = image(code, profile);
        image.cpu.registers.ebx = 0x4ffa;
        image.map(4, 0x8000, true);
        image.data(0x8ff9, &[0xa5; 7]);
        let cpu = completed(&image, code.len());
        check_table(
            engine,
            profile,
            code,
            &image,
            &[DescriptorTableQuery::new(
                DescriptorTable::Interrupt,
                DescriptorTableRegister {
                    base: 0xdead_c0de,
                    limit: 0x7ff,
                },
            )],
            Step {
                cpu,
                ram: &[(0x8ffa, &bytes)],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

fn real(engine: Engine) {
    let profile = ExecutionProfile::Real16;
    for code in [&[0x0f, 0x01, 0x0f][..], &[0x66, 0x0f, 0x01, 0x0f][..]] {
        for offset in [0xfffa, 0xfffb] {
            let mut image = image(code, profile);
            image.cpu.registers.ebx = offset;
            image.map(0xf, 0x8000, true);
            image.data(0x8ff9, &[0xa5; 7]);
            let (cpu, exit) = if offset == 0xfffa {
                let cpu = completed(&image, code.len());
                (cpu, Exit::Dispatch(cpu.eip))
            } else {
                (image.cpu, Exit::GeneralProtection { error: 0 })
            };
            let stored = [(0x8ffa, &[0xff, 3, 0, 0, 0, 0][..])];
            check_table(
                engine,
                profile,
                code,
                &image,
                &[],
                Step {
                    cpu,
                    ram: if offset == 0xfffa { &stored } else { &[] },
                    exit,
                },
            );
        }
    }
}

#[test]
fn protected_sidt_reads_idtr_in_both_operand_sizes() {
    protected(Engine::Wasmtime);
}

#[test]
fn real_sidt_reports_the_active_ivt_and_checks_the_full_span() {
    real(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_idtr_stores() {
    protected(Engine::V8);
    real(Engine::V8);
}
