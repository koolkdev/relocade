#![cfg(not(target_arch = "wasm32"))]
mod support;
use support::Configuration;

use relocade_machine::{Cpu, CpuConfig, CpuStorage, MachineConfig, Memory, MemoryError};
use relocade_x86::{
    Compiler, CpuFeature, CpuModel, CpuState, DescriptorTables, Exception, ExecutionExit,
    ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segment, SegmentDefaultSize,
    SegmentDescriptor, SegmentDescriptorKind, SegmentLimit, SegmentProfile, SegmentRegister,
};

fn create(config: Configuration) -> Cpu {
    pollster::block_on(support::create_cpu(config)).unwrap()
}

fn configuration() -> Configuration {
    support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
        0x10001,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

fn runtime(code: &[u8]) -> Cpu {
    let mut runtime = create(configuration());
    runtime.context().memory.write(0x100, code).unwrap();
    runtime
}

#[test]
fn steps_publish_each_instruction_and_retirement_wraps() {
    let mut runtime = runtime(&[0x40, 0x43, 0xeb, 0xfc]); // INC AX; INC BX; JMP 0100
    let initial = runtime.cpu_state().snapshot();
    drop(runtime.run()); // An unpolled waiter does not execute guest code.
    assert_eq!(runtime.cpu_state().snapshot(), initial);
    for iteration in 1..=3 {
        support::steps(&mut runtime, 1);
        assert_eq!(runtime.cpu_state().eip(), 0x101);
        assert_eq!(runtime.cpu_state().ax(), iteration);
        assert_eq!(runtime.cpu_state().bx(), iteration - 1);
        support::steps(&mut runtime, 2);
        let cpu = runtime.cpu_state().snapshot();
        assert_eq!(
            (cpu.registers.eax, cpu.registers.ebx, cpu.eip),
            (u32::from(iteration), u32::from(iteration), 0x100)
        );
        assert_eq!(cpu.instruction_count, u64::from(iteration) * 3);
    }
    let mut config = configuration();
    let mut initial = *config.cpu.cpu();
    initial.instruction_count = u64::MAX - 1;
    config.cpu = relocade_machine::CpuConfig::new(config.cpu.execution_target(), initial).unwrap();
    let mut wrapping = create(config);
    wrapping
        .memory()
        .view()
        .write(0x100, &[0x40, 0x43, 0xeb, 0xfc])
        .unwrap();
    support::steps(&mut wrapping, 3);
    assert_eq!(wrapping.cpu_state().instruction_count(), 1);
}

#[test]
fn runtimes_keep_private_cpu_and_backing() {
    let mut first = runtime(&[0xb8, 0x34, 0x12, 0xa3, 0, 2, 0xeb, 0]);
    let mut second = runtime(&[0x90]);
    support::steps(&mut first, 3);
    let mut bytes = [0; 2];
    first.context().memory.read(0x200, &mut bytes).unwrap();
    assert_eq!(bytes, [0x34, 0x12]);
    second.context().memory.read(0x200, &mut bytes).unwrap();
    assert_eq!(bytes, [0, 0]);
    assert_eq!(second.context().cpu.instruction_count(), 0);
    assert_eq!(second.context().cpu.eax(), 0);
}

#[test]
fn flat_cpu_queries_and_reloads_its_initial_descriptors() {
    let memory = Memory::new(MachineConfig::flat(0x1000).unwrap()).unwrap();
    memory
        .view()
        .write(
            0,
            &[
                0x31, 0xc0, // XOR EAX,EAX
                0x8e, 0xe0, // MOV FS,AX accepts the null selector
                0xb0, 0x23, // MOV AL,23h
                0x8e, 0xd8, // MOV DS,AX
                0x0f, 0x00, 0xe0, // VERR AX
                0xb0, 0x2b, // MOV AL,2bh (an empty slot)
                0x8e, 0xe0, // MOV FS,AX
            ],
        )
        .unwrap();
    let config = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
    )
    .unwrap();
    let mut cpu = pollster::block_on(Cpu::new_without_io(memory, config)).unwrap();
    assert_eq!(
        pollster::block_on(cpu.run()),
        ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0x28 })
    );
    let state = cpu.cpu_state();
    assert_eq!(state.eip(), 13);
    assert_eq!(state.instruction_count(), 6);
    assert_eq!(state.segment(Segment::Fs), SegmentRegister::unusable(0));
    assert_eq!(
        state.segment(Segment::Ds),
        SegmentRegister::flat_data32(0x23)
    );
    assert_ne!(state.eflags() & (1 << 6), 0);
}

#[test]
fn flat_cpus_share_descriptor_edits_and_keep_their_loaded_caches() {
    let memory = Memory::new(MachineConfig::flat(0x1000).unwrap()).unwrap();
    memory
        .view()
        .write(
            0,
            &[
                0xb0, 0x23, // MOV AL,23h
                0x8e, 0xe0, // MOV FS,AX
                0x0f, 0x03, 0xc8, // LSL ECX,AX
                0x8e, 0xe0, // MOV FS,AX
            ],
        )
        .unwrap();
    let config = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
    )
    .unwrap();
    let mut first = pollster::block_on(Cpu::new_without_io(memory.clone(), config)).unwrap();
    let mut second = pollster::block_on(Cpu::new_without_io(memory.clone(), config)).unwrap();
    support::steps(&mut first, 2);
    support::steps(&mut second, 2);
    {
        let mut tables = memory.descriptor_tables().unwrap();
        let mut changed = *tables.get(0x23).unwrap();
        changed.base = 0x200;
        changed.limit = SegmentLimit::bytes(0xff).unwrap();
        tables.insert(0x23, changed);
    }
    for cpu in [&mut first, &mut second] {
        support::steps(cpu, 1);
        let state = cpu.cpu_state();
        assert_eq!(state.ecx(), 0xff);
        assert_eq!(state.segment(Segment::Fs).base, 0);
        support::steps(cpu, 1);
        assert_eq!(state.segment(Segment::Fs).base, 0x200);
        assert_eq!(state.segment(Segment::Ds).base, 0);
    }
}

#[test]
fn flat_cpus_resolve_thread_selectors_and_stop_before_an_unsupported_reload() {
    let memory = Memory::new(MachineConfig::flat(0x1000).unwrap()).unwrap();
    memory
        .view()
        .write(
            0,
            &[
                0xb8, 0x3b, 0, 0, 0, // MOV EAX,3bh
                0x0f, 0x03, 0xc8, // LSL ECX,AX
                0x8e, 0xe0, // MOV FS,AX
                0x64, 0x8b, 0x15, 0, 0, 0, 0, // MOV EDX,FS:[0]
                0x8e, 0xd8, // MOV DS,AX
                0xf1, // unsupported INT1
                0x0f, 0xb2, 0x0d, 0x80, 0, 0, 0, // LSS ECX,[80h]
            ],
        )
        .unwrap();
    memory
        .view()
        .write(0x80, &[0x78, 0x56, 0x34, 0x12, 0x3b, 0])
        .unwrap();
    let config = CpuConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
    )
    .unwrap();
    let make_cpu = |base: u32, value: u32| {
        memory.view().write(base, &value.to_le_bytes()).unwrap();
        let mut tables = DescriptorTables::flat32();
        tables.insert(
            0x3b,
            SegmentDescriptor::new(
                base,
                SegmentLimit::bytes(base - 1).unwrap(),
                SegmentDescriptorKind::Data {
                    writable: true,
                    expand_down: false,
                },
                SegmentDefaultSize::Bits32,
            ),
        );
        let storage = CpuStorage::new().unwrap().with_descriptor_tables(tables);
        pollster::block_on(storage.into_cpu_without_io(memory.clone(), config)).unwrap()
    };
    let first = make_cpu(0x100, 0x1320);
    let second = make_cpu(0x200, 0x2430);
    for (mut cpu, base, value) in [(first, 0x100, 0x1320), (second, 0x200, 0x2430)] {
        assert_eq!(
            pollster::block_on(cpu.run()),
            ExecutionExit::UnsupportedInstruction {
                eip: 17,
                opcode: 0x8e
            }
        );
        let state = cpu.cpu_state();
        assert_eq!(state.instruction_count(), 4);
        assert_eq!(state.ecx(), base - 1);
        assert_eq!(state.edx(), value);
        assert_eq!(state.segment(Segment::Fs).base, base);
        assert_eq!(
            state.segment(Segment::Ds),
            SegmentRegister::flat_data32(0x23)
        );

        state.set_eip(20);
        assert_eq!(
            cpu.step(),
            ExecutionExit::UnsupportedInstruction {
                eip: 20,
                opcode: 0x0f
            }
        );
        assert_eq!(state.ecx(), base - 1); // LSS leaves its destination and SS unchanged.
        assert_eq!(
            state.segment(Segment::Ss),
            SegmentRegister::flat_data32(0x23)
        );
        assert_eq!(state.instruction_count(), 4);
        state.set_eip(17);

        let replacement = *DescriptorTables::flat32().get(0x23).unwrap();
        cpu.descriptor_tables().unwrap().insert(0x3b, replacement);
        assert_eq!(
            pollster::block_on(cpu.run()),
            ExecutionExit::UnsupportedInstruction {
                eip: 19,
                opcode: 0xf1
            }
        );
        assert_eq!(
            state.segment(Segment::Ds),
            SegmentRegister::flat_data32(0x3b)
        );
        assert_eq!(state.segment(Segment::Fs).base, base);
        state.set_eip(5);
        support::steps(&mut cpu, 1); // LSL now sees the replacement limit.
        assert_eq!(state.ecx(), u32::MAX);
    }
}

#[test]
fn host_backing_access_respects_configured_capacity() {
    let mut runtime = runtime(&[]);
    assert_eq!(runtime.context().memory.backing_size(), 0x10001);
    runtime
        .context()
        .memory
        .write_backing(0x10000, &[0x7b])
        .unwrap();
    let mut byte = [0];
    runtime
        .context()
        .memory
        .read_backing(0x10000, &mut byte)
        .unwrap();
    assert_eq!(byte, [0x7b]);
    assert!(matches!(
        runtime.context().memory.write_backing(0x10000, &[1, 2]),
        Err(MemoryError::BackingRange)
    ));
    assert!(matches!(
        runtime.context().memory.read_backing(0x10001, &mut byte),
        Err(MemoryError::BackingRange)
    ));
    runtime
        .context()
        .memory
        .read_backing(0x10000, &mut byte)
        .unwrap();
    assert_eq!(byte, [0x7b]);
}

#[test]
fn guest_exits_preserve_progress_and_live_code_can_be_replaced() {
    // INC AX; MOV AX,[10000] exceeds the real-mode DS limit.
    let mut runtime = runtime(&[0x40, 0x67, 0xa1, 0, 0, 1, 0]);
    assert_eq!(
        pollster::block_on(runtime.run()),
        ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 })
    );
    assert_eq!(
        (
            runtime.context().cpu.eax(),
            runtime.context().cpu.eip(),
            runtime.context().cpu.instruction_count()
        ),
        (1, 0x101, 1)
    );
    runtime
        .context()
        .memory
        .write(0x101, &[0x40, 0xf1])
        .unwrap(); // INC AX; unsupported INT1
    assert_eq!(
        pollster::block_on(runtime.run()),
        ExecutionExit::UnsupportedInstruction {
            eip: 0x102,
            opcode: 0xf1
        }
    );
    assert_eq!(
        (
            runtime.context().cpu.eax(),
            runtime.context().cpu.instruction_count()
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
        (0x9000..=0x9fff, PhysicalMapping::MMIO),
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
        .memory
        .write(
            0x100,
            &[
                0xa1, 0, 0x80, 0x40, 0xa3, 0, 0x80, 0xe4, 0x80, 0x8b, 0x1e, 0, 0x90, 0xeb, 0,
            ],
        )
        .unwrap();
    runtime
        .context()
        .memory
        .write_backing(0x1000, &[0x34, 0x12])
        .unwrap();
    support::steps(&mut runtime, 6);
    assert_eq!(
        (runtime.context().cpu.eax(), runtime.context().cpu.ebx()),
        (0x12ff, 0xffff)
    );
    let mut bytes = [0; 2];
    runtime.context().memory.read(0x8000, &mut bytes).unwrap();
    assert_eq!(bytes, [0x34, 0x12]);
}

#[test]
fn real_interrupt_executes_handler_and_returns_through_saved_frame() {
    let mut runtime = runtime(&[0xcd, 0x21, 0x90]);
    runtime.context().cpu.set_esp(0xff00);
    runtime
        .context()
        .memory
        .write(0x84, &[0x10, 0, 0, 1])
        .unwrap(); // vector -> 0100:0010
    runtime
        .context()
        .memory
        .write(0x1010, &[0xb8, 0xef, 0xbe, 0xcf])
        .unwrap(); // MOV AX,BEEF; IRET
    support::steps(&mut runtime, 1);
    assert_eq!(
        (
            runtime.context().cpu.segment(Segment::Cs).selector,
            runtime.context().cpu.eip()
        ),
        (0x100, 0x10)
    );
    support::steps(&mut runtime, 2);
    assert_eq!(
        (
            runtime.context().cpu.segment(Segment::Cs).selector,
            runtime.context().cpu.eip(),
            runtime.context().cpu.esp()
        ),
        (0, 0x102, 0xff00)
    );
    assert_eq!(runtime.context().cpu.eax(), 0xbeef);
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
    enabled.context().memory.write(0x100, &code).unwrap();
    disabled.context().memory.write(0x100, &code).unwrap();
    disabled.cpu_state().set_sp(0x8000);
    relocade_machine::real_mode_interrupt::write_vector(
        &mut disabled.memory().view(),
        Exception::InvalidOpcode.vector() as u8,
        relocade_machine::FarPointer {
            segment: 0,
            offset: 0x600,
        },
    );
    assert_eq!(enabled.step(), ExecutionExit::StepComplete);
    assert_eq!(disabled.step(), ExecutionExit::InterruptEntered);
    assert_eq!(disabled.cpu_state().eip(), 0x600);
    assert_eq!(disabled.cpu_state().instruction_count(), 0);
    let frame = relocade_machine::real_mode_interrupt::read_frame(
        &disabled.cpu_state(),
        &disabled.memory().view(),
    );
    assert_eq!(frame.return_address.offset, 0x100);
}
