use wasm86_x86::{CpuState, Gpr32};

use crate::support::{
    conditions::check_conditions,
    machine::{both, Exit, Image, Step},
    step::TestModule,
};

use super::image;

struct RegisterCase {
    name: &'static str,
    code: &'static [u8],
    before: &'static [(Gpr32, u32)],
    after: &'static [(Gpr32, u32)],
    recipe: (u8, u32, u32),
}

fn check_register(case: RegisterCase) -> (Image, CpuState) {
    let mut image = image(case.code);
    for &(register, value) in case.before {
        image.cpu.registers[register] = value;
    }
    let mut cpu = image.cpu;
    for &(register, value) in case.after {
        cpu.registers[register] = value;
    }
    (cpu.flags.kind, cpu.flags.left, cpu.flags.right) = case.recipe;
    cpu.eip += case.code.len() as u32;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        case.name,
        case.code,
        1,
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    (image, cpu)
}

#[test]
fn register_forms_publish_the_old_comparison_for_both_outcomes_at_every_width() {
    for case in [
        RegisterCase {
            name: "byte mismatch replaces AL with old CL",
            code: &[0x0f, 0xb0, 0xd1],
            before: &[],
            after: &[(Gpr32::Eax, 0x4433_2255)],
            recipe: (1, 0x11, 0x55),
        },
        RegisterCase {
            name: "byte match replaces CL with DL",
            code: &[0x0f, 0xb0, 0xd1],
            before: &[(Gpr32::Ecx, 0x8877_6611)],
            after: &[(Gpr32::Ecx, 0x8877_6699)],
            recipe: (1, 0x11, 0x11),
        },
        RegisterCase {
            name: "word mismatch preserves the upper accumulator half",
            code: &[0x66, 0x0f, 0xb1, 0xd1],
            before: &[],
            after: &[(Gpr32::Eax, 0x4433_6655)],
            recipe: (5, 0x2211, 0x6655),
        },
        RegisterCase {
            name: "word match preserves the upper destination half",
            code: &[0x66, 0x0f, 0xb1, 0xd1],
            before: &[(Gpr32::Ecx, 0x8877_2211)],
            after: &[(Gpr32::Ecx, 0x8877_aa99)],
            recipe: (5, 0x2211, 0x2211),
        },
        RegisterCase {
            name: "dword mismatch replaces EAX with old ECX",
            code: &[0x0f, 0xb1, 0xd1],
            before: &[],
            after: &[(Gpr32::Eax, 0x8877_6655)],
            recipe: (9, 0x4433_2211, 0x8877_6655),
        },
        RegisterCase {
            name: "dword match replaces ECX with EDX",
            code: &[0x0f, 0xb1, 0xd1],
            before: &[(Gpr32::Ecx, 0x4433_2211)],
            after: &[(Gpr32::Ecx, 0xccbb_aa99)],
            recipe: (9, 0x4433_2211, 0x4433_2211),
        },
    ] {
        check_register(case);
    }
}

#[test]
fn accumulator_destinations_match_and_take_the_source_value() {
    for case in [
        RegisterCase {
            name: "AL destination takes DL",
            code: &[0x0f, 0xb0, 0xd0],
            before: &[],
            after: &[(Gpr32::Eax, 0x4433_2299)],
            recipe: (1, 0x11, 0x11),
        },
        RegisterCase {
            name: "AX destination takes DX",
            code: &[0x66, 0x0f, 0xb1, 0xd0],
            before: &[],
            after: &[(Gpr32::Eax, 0x4433_aa99)],
            recipe: (5, 0x2211, 0x2211),
        },
        RegisterCase {
            name: "EAX destination takes EDX",
            code: &[0x0f, 0xb1, 0xd0],
            before: &[],
            after: &[(Gpr32::Eax, 0xccbb_aa99)],
            recipe: (9, 0x4433_2211, 0x4433_2211),
        },
    ] {
        check_register(case);
    }
}

#[test]
fn high_bytes_and_source_aliases_use_values_from_before_the_comparison() {
    for case in [
        RegisterCase {
            name: "AH mismatch replaces AL without replacing AH",
            code: &[0x0f, 0xb0, 0xcc],
            before: &[],
            after: &[(Gpr32::Eax, 0x4433_2222)],
            recipe: (1, 0x11, 0x22),
        },
        RegisterCase {
            name: "AH match changes AH while preserving AL",
            code: &[0x0f, 0xb0, 0xcc],
            before: &[(Gpr32::Eax, 0x4433_1111)],
            after: &[(Gpr32::Eax, 0x4433_5511)],
            recipe: (1, 0x11, 0x11),
        },
        RegisterCase {
            name: "AH source supplies its old value to matching CL",
            code: &[0x0f, 0xb0, 0xe1],
            before: &[(Gpr32::Ecx, 0x8877_6611)],
            after: &[(Gpr32::Ecx, 0x8877_6622)],
            recipe: (1, 0x11, 0x11),
        },
        RegisterCase {
            name: "an accumulator source does not overwrite a mismatch destination",
            code: &[0x0f, 0xb1, 0xc1],
            before: &[],
            after: &[(Gpr32::Eax, 0x8877_6655)],
            recipe: (9, 0x4433_2211, 0x8877_6655),
        },
        RegisterCase {
            name: "a matching source and destination still publish the comparison",
            code: &[0x0f, 0xb1, 0xc9],
            before: &[(Gpr32::Ecx, 0x4433_2211)],
            after: &[],
            recipe: (9, 0x4433_2211, 0x4433_2211),
        },
    ] {
        check_register(case);
    }
}

#[test]
fn setcc_consumes_equal_and_overflowing_comparison_flags() {
    for (case, conditions) in [
        (
            RegisterCase {
                name: "equal dword comparison",
                code: &[0x0f, 0xb1, 0xd1],
                before: &[(Gpr32::Ecx, 0x4433_2211)],
                after: &[(Gpr32::Ecx, 0xccbb_aa99)],
                recipe: (9, 0x4433_2211, 0x4433_2211),
            },
            0x665a,
        ),
        (
            RegisterCase {
                name: "byte comparison overflows with an unsigned borrow",
                code: &[0x0f, 0xb0, 0xd1],
                before: &[(Gpr32::Eax, 0x4433_227f), (Gpr32::Ecx, 0x8877_66ff)],
                after: &[(Gpr32::Eax, 0x4433_22ff)],
                recipe: (1, 0x7f, 0xff),
            },
            0xa965,
        ),
    ] {
        let name = case.name;
        let code = case.code;
        let (mut image, cpu) = check_register(case);
        check_conditions(
            TestModule::interpreter(),
            name,
            code,
            &mut image,
            &cpu,
            conditions,
        );
    }
}

#[test]
fn memory_comparisons_preserve_old_addresses_and_aliased_sources() {
    let code = [0x66, 0x0f, 0xb1, 0x10];
    let mut initial = image(&code);
    initial.cpu.registers.eax = 0x1111_4020;
    initial.map(0x11114, 0x8000, true);
    initial.data(0x8020, &[0xdc, 0xfe]);
    let mut cpu = initial.cpu;
    cpu.registers.eax = 0x1111_fedc;
    (cpu.flags.kind, cpu.flags.left, cpu.flags.right) = (5, 0x4020, 0xfedc);
    cpu.eip = 0x1004;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        "word mismatch keeps the old EAX address and upper half",
        &code,
        1,
        &initial,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );

    let code = [0x0f, 0xb1, 0x1b];
    let mut initial = image(&code);
    initial.cpu.registers.ebx = 0x4020;
    initial.map(4, 0x8000, true);
    initial.data(0x8020, &[0x11, 0x22, 0x33, 0x44]);
    let mut cpu = initial.cpu;
    (cpu.flags.kind, cpu.flags.left, cpu.flags.right) = (9, 0x4433_2211, 0x4433_2211);
    cpu.eip = 0x1003;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        "matching memory destination takes its EBX address source",
        &code,
        1,
        &initial,
        &[Step {
            cpu,
            ram: &[(0x8020, &[0x20, 0x40, 0, 0])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );

    let code = [0x0f, 0xb1, 0x4c, 0x8b, 0x20];
    let mut initial = image(&code);
    initial.cpu.registers.ebx = 0x4000;
    initial.cpu.registers.ecx = 8;
    initial.map(4, 0x8000, true);
    initial.data(0x8040, &[0x11, 0x22, 0x33, 0x44]);
    let mut cpu = initial.cpu;
    (cpu.flags.kind, cpu.flags.left, cpu.flags.right) = (9, 0x4433_2211, 0x4433_2211);
    cpu.eip = 0x1005;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        "matching memory destination takes its scaled ECX index source",
        &code,
        1,
        &initial,
        &[Step {
            cpu,
            ram: &[(0x8040, &[8, 0, 0, 0])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}
