#![cfg(not(target_arch = "wasm32"))]
mod support;
use support::Configuration;

use relocade_machine::Cpu;
use relocade_machine::MemoryError;
use relocade_x86::{
    Compiler, CpuFeature, CpuModel, CpuState, Exception, ExecutionExit, ExecutionProfile,
    PhysicalMapping, PhysicalMemoryMap, Segment, Segments,
};

fn create(config: Configuration) -> Cpu {
    pollster::block_on(support::create_cpu((), config)).unwrap()
}

fn configuration() -> Configuration {
    support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10001,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

fn runtime(code: &[u8]) -> Cpu {
    let mut runtime = create(configuration());
    runtime.context().memory().write(0x100, code).unwrap();
    runtime
}

#[test]
fn budget_checks_resume_at_block_boundaries() {
    let mut runtime = runtime(&[0x40, 0x43, 0xeb, 0xfc]); // INC AX; INC BX; JMP 0100
    let initial = runtime.context().cpu().snapshot();
    assert_eq!(runtime.run(0).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().snapshot(), initial);
    for iteration in 1..=3 {
        assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
        let cpu = runtime.context().cpu().snapshot();
        assert_eq!(
            (cpu.registers.eax, cpu.registers.ebx, cpu.eip),
            (iteration, iteration, 0x100)
        );
        assert_eq!(cpu.instruction_count, u64::from(iteration) * 3);
    }
    runtime.context().cpu().set_instruction_count(u64::MAX - 3);
    runtime.context().cpu().set_eax(40);
    assert_eq!(runtime.run(10).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu().instruction_count(), u64::MAX);
    assert_eq!(runtime.context().cpu().eax(), 41);
}

#[test]
fn runtimes_keep_private_cpu_and_backing() {
    let mut first = runtime(&[0xb8, 0x34, 0x12, 0xa3, 0, 2, 0xeb, 0]);
    let mut second = runtime(&[0x90]);
    assert_eq!(first.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    let mut bytes = [0; 2];
    first.context().memory().read(0x200, &mut bytes).unwrap();
    assert_eq!(bytes, [0x34, 0x12]);
    second.context().memory().read(0x200, &mut bytes).unwrap();
    assert_eq!(bytes, [0, 0]);
    assert_eq!(second.context().cpu().instruction_count(), 0);
    assert_eq!(second.context().cpu().eax(), 0);
}

#[test]
fn host_backing_access_respects_configured_capacity() {
    let mut runtime = runtime(&[]);
    assert_eq!(runtime.context().memory().backing_size(), 0x10001);
    runtime
        .context()
        .memory()
        .write_backing(0x10000, &[0x7b])
        .unwrap();
    let mut byte = [0];
    runtime
        .context()
        .memory()
        .read_backing(0x10000, &mut byte)
        .unwrap();
    assert_eq!(byte, [0x7b]);
    assert!(matches!(
        runtime.context().memory().write_backing(0x10000, &[1, 2]),
        Err(MemoryError::BackingRange)
    ));
    assert!(matches!(
        runtime.context().memory().read_backing(0x10001, &mut byte),
        Err(MemoryError::BackingRange)
    ));
    runtime
        .context()
        .memory()
        .read_backing(0x10000, &mut byte)
        .unwrap();
    assert_eq!(byte, [0x7b]);
}

#[test]
fn guest_exits_preserve_progress_and_live_code_can_be_replaced() {
    let mut runtime = runtime(&[0x40, 0x0f, 0x0b]); // INC AX; UD2
    assert_eq!(
        runtime.run(u64::MAX).unwrap(),
        ExecutionExit::Exception(Exception::InvalidOpcode)
    );
    assert_eq!(
        (
            runtime.context().cpu().eax(),
            runtime.context().cpu().eip(),
            runtime.context().cpu().instruction_count()
        ),
        (1, 0x101, 1)
    );
    runtime
        .context()
        .memory()
        .write(0x101, &[0x40, 0xf4])
        .unwrap(); // INC AX; unsupported HLT
    assert_eq!(
        runtime.run(u64::MAX).unwrap(),
        ExecutionExit::UnsupportedInstruction {
            eip: 0x102,
            opcode: 0xf4
        }
    );
    assert_eq!(
        (
            runtime.context().cpu().eax(),
            runtime.context().cpu().instruction_count()
        ),
        (2, 2)
    );
}

#[test]
fn guest_access_uses_the_map_and_open_bus() {
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x2000,
            },
        ),
        (
            0x8000..=0x8fff,
            PhysicalMapping::Rom {
                backing_offset: 0x1000,
            },
        ),
        (0x9000..=0x9fff, PhysicalMapping::Mmio),
    ])
    .unwrap();
    let initial = configuration();
    let config = support::configuration(
        initial.cpu.execution_target(),
        *initial.cpu.cpu(),
        0x3000,
        map,
    )
    .unwrap();
    let mut runtime = create(config);
    // MOV AX,[8000]; INC AX; MOV [8000],AX; IN AL,80; MOV BX,[9000]; JMP next
    runtime
        .context()
        .memory()
        .write(
            0x100,
            &[
                0xa1, 0, 0x80, 0x40, 0xa3, 0, 0x80, 0xe4, 0x80, 0x8b, 0x1e, 0, 0x90, 0xeb, 0,
            ],
        )
        .unwrap();
    runtime
        .context()
        .memory()
        .write_backing(0x1000, &[0x34, 0x12])
        .unwrap();
    assert_eq!(runtime.run(6).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(
        (runtime.context().cpu().eax(), runtime.context().cpu().ebx()),
        (0x12ff, 0xffff)
    );
    let mut bytes = [0; 2];
    runtime.context().memory().read(0x8000, &mut bytes).unwrap();
    assert_eq!(bytes, [0x34, 0x12]);
}

#[test]
fn real_interrupt_executes_handler_and_returns_through_saved_frame() {
    let mut runtime = runtime(&[0xcd, 0x21, 0x90]);
    runtime.context().cpu().set_esp(0xff00);
    runtime
        .context()
        .memory()
        .write(0x84, &[0x10, 0, 0, 1])
        .unwrap(); // vector -> 0100:0010
    runtime
        .context()
        .memory()
        .write(0x1010, &[0xb8, 0xef, 0xbe, 0xcf])
        .unwrap(); // MOV AX,BEEF; IRET
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(
        (
            runtime.context().cpu().segment(Segment::Cs).selector,
            runtime.context().cpu().eip()
        ),
        (0x100, 0x10)
    );
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(
        (
            runtime.context().cpu().segment(Segment::Cs).selector,
            runtime.context().cpu().eip(),
            runtime.context().cpu().esp()
        ),
        (0, 0x102, 0xff00)
    );
    assert_eq!(runtime.context().cpu().eax(), 0xbeef);
}

#[test]
fn prepared_code_uses_configured_cpu_features() {
    let initial = configuration();
    let target = Compiler::new(ExecutionProfile::Real16)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false))
        .target();
    let disabled = support::configuration(
        target,
        *initial.cpu.cpu(),
        initial.machine.memory_size(),
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut enabled = create(initial);
    let mut disabled = create(disabled);
    let code = [0x0f, 0x44, 0xc0, 0xeb, 0]; // CMOVE AX,AX; JMP next
    enabled.context().memory().write(0x100, &code).unwrap();
    disabled.context().memory().write(0x100, &code).unwrap();
    assert_eq!(enabled.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(
        disabled.run(1).unwrap(),
        ExecutionExit::Exception(Exception::InvalidOpcode)
    );
}
