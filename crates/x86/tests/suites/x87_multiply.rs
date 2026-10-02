//! Register multiplication preserves extended precision, status and stack effects.

#[path = "x87_multiply/exceptions.rs"]
mod exceptions;
#[path = "x87_multiply/rounding.rs"]
mod rounding;
#[path = "x87_multiply/specialization.rs"]
mod specialization;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Step},
    step::Engine,
    x87::{
        complete_x87, dispatch, set_control, stack_image, status, write_register_bits, INDEFINITE,
    },
};
use wasm86_x86::{CpuState, SegmentProfile};

const LEADING: u64 = 1 << 63;
const PE: u16 = 0x20;
const C1: u16 = 0x200;
const PENDING: u16 = 0x8080;

#[derive(Debug)]
struct Case {
    left: (u64, u16),
    right: (u64, u16),
    control: u16,
    result: Option<(u64, u16)>,
    flags: u16,
}

fn tag(value: (u64, u16)) -> u16 {
    match (value.0, value.1 & 0x7fff) {
        (0, 0) => 1,
        (_, 0 | 0x7fff) => 2,
        (significand, _) if significand < LEADING => 2,
        _ => 0,
    }
}

fn write_value(cpu: &mut CpuState, slot: usize, value: (u64, u16)) {
    write_register_bits(cpu, slot, value);
    cpu.x87.tag_word = (cpu.x87.tag_word & !(3 << (slot * 2))) | (tag(value) << (slot * 2));
}

// Numerical and exception cases use FMULP ST1, ST0 to check write/pop together.
// The form tests below own all other destinations and aliases.
fn check_product(checks: &mut ImageSequences, name: &str, case: Case) {
    let code = [0xde, 0xc9, 0xdf, 0xe0, 0x9b];
    let mut image = stack_image(&code, 7, 0x3ffc);
    image.cpu.x87.status.precision = 0;
    set_control(&mut image.cpu.x87.control, case.control);
    write_value(&mut image.cpu, 0, case.left);
    write_value(&mut image.cpu, 7, case.right);
    let mut product = complete_x87(image.cpu, 2, 0x06c9);
    product.x87.status = status(0x4500 | case.flags);
    if let Some(result) = case.result {
        write_value(&mut product, 0, result);
        product.x87.tag_word |= 0xc000;
    } else {
        product.x87.status.top = 7;
    }
    let mut observed = product;
    observed.registers.eax =
        0x1111_0000 | u32::from(0x4500 | case.flags) | (u32::from(product.x87.status.top) << 11);
    observed.eip += 2;
    observed.instruction_count += 1;
    let mut waited = observed;
    let exit = if case.flags & PENDING != 0 {
        Exit::FloatingPoint
    } else {
        waited.eip += 1;
        waited.instruction_count += 1;
        Exit::Dispatch(waited.eip)
    };
    checks.check(
        &format!("{name}: {case:x?}"),
        &code,
        &image,
        &[
            dispatch(product),
            dispatch(observed),
            Step {
                cpu: waited,
                ram: &[],
                exit,
            },
        ],
    );
}

fn register_forms(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (code, top, other, destination, pop, result) in [
        (
            [0xd8, 0xca],
            7,
            1,
            7,
            false,
            (0xc000_0000_0000_0000, 0xc001),
        ),
        (
            [0xdc, 0xca],
            7,
            1,
            1,
            false,
            (0xc000_0000_0000_0000, 0xc001),
        ),
        ([0xde, 0xca], 7, 1, 1, true, (0xc000_0000_0000_0000, 0xc001)),
        ([0xd8, 0xc8], 3, 3, 3, false, (LEADING, 0x4001)),
        ([0xdc, 0xc8], 3, 3, 3, false, (LEADING, 0x4001)),
        ([0xde, 0xc8], 7, 7, 7, true, (LEADING, 0x4001)),
    ] {
        let mut image = stack_image(&code, top, 0);
        write_value(&mut image.cpu, other, (0xc000_0000_0000_0000, 0x4000)); // 3
        write_value(&mut image.cpu, top as usize, (LEADING, 0xc000)); // -2
        let opcode = (u16::from(code[0] & 7) << 8) | u16::from(code[1]);
        let mut product = complete_x87(image.cpu, 2, opcode);
        product.x87.status.c1 = 0;
        write_value(&mut product, destination, result);
        if pop {
            product.x87.tag_word |= 3 << (top * 2);
            product.x87.status.top = (top + 1) & 7;
        }
        checks.check(
            "register destinations use pre-pop TOP",
            &code,
            &image,
            &[dispatch(product)],
        );
    }
}

fn live_products(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Loading exact binary32 values, multiplying in registers, then storing a
    // binary64 result exercises the same value across representation boundaries.
    let code = [
        0xd9, 0x05, 0, 0x40, 0, 0, 0xd9, 0x05, 4, 0x40, 0, 0, 0xde, 0xc9, 0xdd, 0x1d, 8, 0x40, 0, 0,
    ];
    let mut image = stack_image(&code, 0, 0xffff);
    image.map(4, 0x8000, true);
    image.data(0x8000, &[0, 0, 0xc0, 0x3f, 0, 0, 0x20, 0x40]); // 1.5, 2.5
    let mut first = complete_x87(image.cpu, 6, 0x0105);
    first.x87.status.top = 7;
    first.x87.status.c1 = 0;
    first.x87.data_offset = 0x4000;
    first.x87.data_selector = 0x23;
    write_value(&mut first, 7, (0xc000_0000_0000_0000, 0x3fff));
    let mut second = complete_x87(first, 6, 0x0105);
    second.x87.status.top = 6;
    second.x87.data_offset = 0x4004;
    write_value(&mut second, 6, (0xa000_0000_0000_0000, 0x4000));
    let mut product = complete_x87(second, 2, 0x06c9);
    product.x87.status.top = 7;
    product.x87.tag_word |= 3 << 12;
    write_value(&mut product, 7, (0xf000_0000_0000_0000, 0x4000));
    let mut stored = complete_x87(product, 6, 0x051d);
    stored.x87.status.top = 0;
    stored.x87.tag_word = 0xffff;
    stored.x87.data_offset = 0x4008;
    checks.check(
        "load, multiply and store retain the live product",
        &code,
        &image,
        &[
            dispatch(first),
            dispatch(second),
            dispatch(product),
            Step {
                cpu: stored,
                ram: &[(0x8008, &[0, 0, 0, 0, 0, 0, 0x0e, 0x40])], // 3.75
                exit: Exit::Dispatch(stored.eip),
            },
        ],
    );
}

fn freed_product_stack_fault(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // FMUL establishes a normal value. FFREE changes occupancy without changing
    // its payload; FST must replace both its value and classification on fault.
    let code = [
        0xd8, 0xc9, // FMUL ST0, ST1
        0xdd, 0xc0, // FFREE ST0
        0xdd, 0xd0, // FST ST0
        0xdd, 0x1d, 0, 0x40, 0, 0, // FSTP m64real
    ];
    let mut image = stack_image(&code, 0, 0xfff0);
    image.map(4, 0x8000, true);
    write_value(&mut image.cpu, 0, (LEADING, 0x4000)); // 2
    write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x4000)); // 3
    let mut product = complete_x87(image.cpu, 2, 0x00c9);
    product.x87.status.c1 = 0;
    write_value(&mut product, 0, (0xc000_0000_0000_0000, 0x4001)); // 6
    let mut freed = complete_x87(product, 2, 0x05c0);
    freed.x87.tag_word |= 3;
    let mut indefinite = complete_x87(freed, 2, 0x05d0);
    indefinite.x87.status.invalid = 1;
    indefinite.x87.status.stack_fault = 1;
    write_value(&mut indefinite, 0, INDEFINITE);
    let mut stored = complete_x87(indefinite, 6, 0x051d);
    stored.x87.status.top = 1;
    stored.x87.tag_word |= 3;
    stored.x87.data_offset = 0x4000;
    stored.x87.data_selector = 0x23;
    checks.check(
        "a freed normal product becomes indefinite on masked stack fault",
        &code,
        &image,
        &[
            dispatch(product),
            dispatch(freed),
            dispatch(indefinite),
            Step {
                cpu: stored,
                ram: &[(0x8000, &0xfff8_0000_0000_0000_u64.to_le_bytes())],
                exit: Exit::Dispatch(stored.eip),
            },
        ],
    );
}

test_frontends!(forms, register_forms);
test_frontends!(live_values, live_products);
test_frontends!(freed_product, freed_product_stack_fault);
