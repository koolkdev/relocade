use super::*;

fn pending_flags(engine: Engine) {
    let code = [0x63, 0xc8]; // ARPL AX,CX.
    for profile in PROFILES {
        for (selector, adjusted) in [(0x20, true), (0x23, false)] {
            let mut image = image(&code, profile);
            image.cpu.registers.eax = 0xabcd_0000 | selector;
            image.cpu.registers.ecx = 0x9876_0003;
            image.cpu.flags.status_source.kind = 10; // ADD32: 0x7fffffff + 1.
            image.cpu.flags.status_source.left = 0x7fff_ffff;
            image.cpu.flags.status_source.right = 1;
            let mut cpu = completed(&image, code.len(), adjusted);
            cpu.registers.eax = 0xabcd_0023;
            cpu.flags.status_source.kind = 0;
            cpu.flags.bytes.cf = 0;
            cpu.flags.bytes.pf = 1;
            cpu.flags.bytes.af = 1;
            cpu.flags.bytes.sf = 1;
            cpu.flags.bytes.of = 1;
            check_one(
                engine,
                profile,
                &code,
                &image,
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
}

#[test]
fn adjustment_and_no_change_replace_zf_while_preserving_other_pending_flags() {
    pending_flags(Engine::Wasmtime);
}

fn continuing_block(engine: Engine) {
    // ARPL AX,CX; SETZ DL; ARPL AX,CX; SETZ BL.
    let code = [0x63, 0xc8, 0x0f, 0x94, 0xc2, 0x63, 0xc8, 0x0f, 0x94, 0xc3];
    for profile in PROFILES {
        let mut image = image(&code, profile);
        image.cpu.registers.eax = 0xabcd_0020;
        image.cpu.registers.ecx = 0x9876_0003;
        let mut first = completed(&image, 2, true);
        first.registers.eax = 0xabcd_0023;
        let mut second = first;
        second.registers.edx = (second.registers.edx & !0xff) | 1;
        second.eip += 3;
        second.instruction_count += 1;
        let mut third = second;
        third.flags.bytes.zf = 0;
        third.eip += 2;
        third.instruction_count += 1;
        let mut fourth = third;
        fourth.registers.ebx &= !0xff;
        fourth.eip += 3;
        fourth.instruction_count += 1;
        let step = |cpu: CpuState| Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        };
        assert_eq!(
            engine.observe(
                TestModule::interpreter_with_profile(profile),
                &image.input(),
                4,
            ),
            expected(
                &image,
                &[step(first), step(second), step(third), step(fourth)]
            )
        );
        let mut blocks = BlockModules::default();
        let block = blocks.get(&image.cpu, &code, 4, profile);
        assert_eq!(
            engine.observe(block, &image.input(), 1),
            expected(&image, &[step(fourth)])
        );
    }
}

#[test]
fn a_block_continues_after_arpl_and_consumers_observe_each_zf_result() {
    continuing_block(Engine::Wasmtime);
}

fn later_fault(engine: Engine) {
    let profile = SegmentProfile::Flat32;
    let code = [0x63, 0xc8, 0x63, 0x0b]; // ARPL AX,CX; ARPL [EBX],CX (unmapped).
    let mut image = image(&code, profile);
    image.cpu.registers.eax = 0xabcd_0020;
    image.cpu.registers.ecx = 3;
    image.cpu.registers.ebx = 0x4000;
    let mut cpu = completed(&image, 2, true);
    cpu.registers.eax = 0xabcd_0023;
    let fault = || Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x4000,
            error: 2,
        },
    };
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 2),
        expected(
            &image,
            &[
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                },
                fault(),
            ],
        )
    );
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 2, profile);
    assert_eq!(
        engine.observe(block, &image.input(), 1),
        expected(&image, &[fault()])
    );
}

#[test]
fn a_later_write_fault_preserves_the_completed_adjustment_and_its_retirement() {
    later_fault(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_arpl_pending_flags_continuation_and_restart() {
    pending_flags(Engine::V8);
    continuing_block(Engine::V8);
    later_fault(Engine::V8);
}
