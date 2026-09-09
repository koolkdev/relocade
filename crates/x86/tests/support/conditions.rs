use wasm86_x86::CpuState;

use super::{
    machine::{both, Exit, Image, Step},
    step::TestModule,
};

pub(crate) fn check_conditions(
    step: &TestModule,
    name: &str,
    prefix: &[u8],
    image: &mut Image,
    prefix_cpu: &CpuState,
    conditions: u16,
) {
    let mut code = prefix.to_vec();
    for condition in 0..16 {
        // ModRM.reg is ignored by SETcc. Every possible value appears here.
        code.extend_from_slice(&[
            0x0f,
            0x90 + condition,
            0x47 | ((condition & 7) << 3),
            condition,
        ]);
    }
    image.data(0x3000, &code);
    image.cpu.registers.edi = 0x6000;
    image.map(6, 0xa000, true);
    image.data(0x9fff, &[0xa5; 18]);
    let results = (0..16)
        .map(|condition| [((conditions >> condition) & 1) as u8])
        .collect::<Vec<_>>();
    let writes = results
        .iter()
        .enumerate()
        .map(|(condition, result)| [(0xa000 + condition as u32, result.as_slice())])
        .collect::<Vec<_>>();
    let mut steps = Vec::new();
    let mut cpu = *prefix_cpu;
    cpu.registers.edi = 0x6000;
    if !prefix.is_empty() {
        cpu.eip = 0x1000 + prefix.len() as u32;
        cpu.instruction_count = 0;
        steps.push(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        });
    }
    for (condition, writes) in writes.iter().enumerate() {
        cpu.eip = 0x1000 + prefix.len() as u32 + 4 * (condition as u32 + 1);
        cpu.instruction_count = condition as u32 + u32::from(!prefix.is_empty());
        steps.push(Step {
            cpu,
            ram: writes,
            exit: Exit::Dispatch(cpu.eip),
        });
    }
    both(step, name, &code, steps.len() as u32, image, &steps);
}
