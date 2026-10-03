//! Register arithmetic checks a destination in ST1, a pop, status observation and FWAIT.

use super::*;
use crate::support::execution::ImageSequences;

#[derive(Debug)]
pub(crate) struct ArithmeticCase {
    pub(crate) left: (u64, u16),
    pub(crate) right: (u64, u16),
    pub(crate) control: u16,
    pub(crate) result: Option<(u64, u16)>,
    pub(crate) flags: u16,
}

pub(crate) fn check_arithmetic(
    checks: &mut ImageSequences,
    instruction: [u8; 2],
    name: &str,
    case: ArithmeticCase,
) {
    let code = [instruction[0], instruction[1], 0xdf, 0xe0, 0x9b];
    let mut image = stack_image(&code, 7, 0x3ffc);
    image.cpu.x87.status.precision = 0;
    set_control(&mut image.cpu.x87.control, case.control);
    write_value(&mut image.cpu, 0, case.left);
    write_value(&mut image.cpu, 7, case.right);
    let opcode = (u16::from(instruction[0] & 7) << 8) | u16::from(instruction[1]);
    let mut result = complete_x87(image.cpu, 2, opcode);
    result.x87.status = status(0x4500 | case.flags);
    if let Some(bits) = case.result {
        write_value(&mut result, 0, bits);
        result.x87.tag_word |= 0xc000;
    } else {
        result.x87.status.top = 7;
    }
    let mut observed = result;
    observed.registers.eax =
        0x1111_0000 | u32::from(0x4500 | case.flags) | (u32::from(result.x87.status.top) << 11);
    observed.eip += 2;
    observed.instruction_count += 1;
    let mut waited = observed;
    let exit = if case.flags & 0x8080 != 0 {
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
            dispatch(result),
            dispatch(observed),
            Step {
                cpu: waited,
                ram: &[],
                exit,
            },
        ],
    );
}
