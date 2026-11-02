#![cfg(feature = "worker")]

mod support;

use relocade_machine::{
    CodeSnapshot, CompiledCode, Cpu, CpuConfig, Device, DeviceConnection, Hook, IoBus, Memory,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segment, SegmentRegister, Segments,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn create_cpu(io: IoBus) -> Cpu {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..Default::default()
        },
        0x10000,
        PhysicalMemoryMap::new([
            (0..=0x7fff, PhysicalMapping::Ram { backing_offset: 0 }),
            (0x8000..=0x8fff, PhysicalMapping::Ram { backing_offset: 0 }),
        ])
        .unwrap(),
    )
    .unwrap();
    Cpu::new(Memory::new(config.machine).unwrap(), config.cpu, io)
        .await
        .unwrap()
}

async fn compile(cpu: &Cpu, start_eip: u32, bytes: &[u8], instruction_limit: u32) -> CompiledCode {
    cpu.compiler_worker()
        .compile_blocks(vec![CodeSnapshot {
            start_eip,
            bytes: bytes.to_vec(),
            instruction_limit,
        }])
        .await
        .unwrap()
}

async fn admission_and_host_writes() {
    let mut cpu = create_cpu(Default::default()).await;
    let memory = cpu.memory();
    let code = compile(&cpu, 0x100, &[0x40, 0xeb, 0xfd], 1).await;
    memory.view().write(0x100, &[0x43, 0xeb, 0xfd]).unwrap();
    assert!(
        !cpu.install_code(&code, 0).await.unwrap(),
        "stale bytes are rejected"
    );
    memory.view().write(0x100, &[0x40, 0xeb, 0]).unwrap();
    assert!(
        cpu.install_code(&code, 0).await.unwrap(),
        "unused suffix may differ"
    );
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        cpu.cpu_state().eip(),
        0x101,
        "compiled instruction boundary"
    );
    assert_eq!(cpu.cpu_state().ax(), 1);

    memory.view().write_u8(0x8100, 0x43).unwrap();
    cpu.cpu_state().set_eip(0x100);
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        cpu.cpu_state().eip(),
        0x103,
        "alias write retired the entry"
    );
    assert_eq!(cpu.cpu_state().ax(), 1);
    assert_eq!(cpu.cpu_state().bx(), 1);

    memory.view().write_u8(0x100, 0x40).unwrap();
    assert!(cpu.install_code(&code, 0).await.unwrap());
    use relocade_wasm_runtime::WasmSharedMemory;
    memory.backing().write(0x100, &[0x43]);
    memory.invalidate_code();
    cpu.cpu_state().set_eip(0x100);
    cpu.run(1).await;
    assert_eq!(cpu.cpu_state().eip(), 0x103);
    assert_eq!(cpu.cpu_state().bx(), 2);

    // A second, sequential CPU sees code-page flags even though it owns no code.
    memory.view().write_u8(0x100, 0x40).unwrap();
    assert!(cpu.install_code(&code, 0).await.unwrap());
    memory
        .view()
        .write(0x2000, &[0xc6, 0x06, 0, 0x81, 0x43, 0xeb, 0xf9])
        .unwrap();
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x2000,
            segments: Segments::real_mode(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut writer = Cpu::new(memory.clone(), config, Default::default())
        .await
        .unwrap();
    writer.run(1).await;
    cpu.cpu_state().set_eip(0x100);
    cpu.run(1).await;
    assert_eq!(cpu.cpu_state().ax(), 1);
    assert_eq!(cpu.cpu_state().bx(), 3);
    assert_eq!(cpu.cpu_state().eip(), 0x103);

    // The same linear address can be reached with another CS-relative EIP.
    memory.view().write_u8(0x100, 0x40).unwrap();
    assert!(cpu.install_code(&code, 0).await.unwrap());
    cpu.cpu_state()
        .set_segment(Segment::Cs, SegmentRegister::real_mode(Segment::Cs, 0x10));
    cpu.cpu_state().set_eip(0);
    cpu.run(1).await;
    assert_eq!(
        cpu.cpu_state().eip(),
        3,
        "alias must not use captured EIP 0100"
    );
}

async fn self_modification_restarts_before_effects() {
    let mut cpu = create_cpu(Default::default()).await;
    // MOV byte [8105],43 changes the following INC AX through an alias to INC BX.
    let bytes = [0xc6, 0x06, 0x05, 0x81, 0x43, 0x40, 0xeb, 0xf8];
    cpu.memory().view().write(0x100, &bytes).unwrap();
    let code = compile(&cpu, 0x100, &bytes, 10).await;
    assert!(cpu.install_code(&code, 0).await.unwrap());
    assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(cpu.cpu_state().ax(), 0);
    assert_eq!(cpu.cpu_state().bx(), 1);
    assert_eq!(cpu.cpu_state().instruction_count(), 3);

    let bytes = [0xf3, 0xa4, 0x40, 0xeb, 0xfb]; // REP MOVSB; INC AX; JMP 0100
    cpu.memory().view().write(0x100, &bytes).unwrap();
    cpu.memory().view().write_u8(0x3000, 0x43).unwrap();
    cpu.cpu_state().set_si(0x3000);
    cpu.cpu_state().set_di(0x102);
    cpu.cpu_state().set_cx(1);
    let code = compile(&cpu, 0x100, &bytes, 10).await;
    assert!(cpu.install_code(&code, 0).await.unwrap());
    cpu.run(1).await;
    assert_eq!(cpu.cpu_state().ax(), 0);
    assert_eq!(cpu.cpu_state().bx(), 2);
    assert_eq!(cpu.cpu_state().cx(), 0);
    assert_eq!(cpu.cpu_state().si(), 0x3001);
    assert_eq!(cpu.cpu_state().di(), 0x103);

    // A whole direct RAM span still runs compiled, including its repeated stores.
    let bytes = [0xf3, 0xa4, 0xeb, 0xfc];
    cpu.memory().view().write(0x100, &bytes).unwrap();
    cpu.cpu_state().set_si(0x3000);
    cpu.cpu_state().set_di(0x4000);
    cpu.cpu_state().set_cx(4);
    let code = compile(&cpu, 0x100, &bytes, 1).await;
    assert!(cpu.install_code(&code, 0).await.unwrap());
    cpu.run(1).await;
    assert_eq!(cpu.cpu_state().eip(), 0x102);
    assert_eq!(cpu.cpu_state().cx(), 0);
    assert_eq!(cpu.memory().view().read_u8(0x4000).unwrap(), 0x43);

    // A partial instruction must not replay an earlier device transfer when a
    // later stack slot reaches a tracked code page.
    struct StackDevice(Rc<Cell<u32>>);
    impl Device for StackDevice {
        fn write_mmio(&mut self, _: u32, _: u8, _: u64) {
            self.0.set(self.0.get() + 1);
        }
    }
    let writes = Rc::new(Cell::new(0));
    let io = IoBus::new([DeviceConnection {
        device: Rc::new(RefCell::new(StackDevice(writes.clone()))),
        ports: vec![],
        mmio: std::iter::once(0x2000..=0x2fff).collect(),
    }]);
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..Default::default()
        },
        0x2000,
        PhysicalMemoryMap::new([
            (0..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 }),
            (0x2000..=0x2fff, PhysicalMapping::Mmio),
        ])
        .unwrap(),
    )
    .unwrap();
    let mut cpu = Cpu::new(Memory::new(config.machine).unwrap(), config.cpu, io)
        .await
        .unwrap();
    let bytes = [0x60, 0xeb, 0xfd];
    cpu.memory().view().write(0x100, &bytes).unwrap();
    cpu.memory().view().write(0x1100, &[0x40, 0xeb, 0]).unwrap();
    let watched = compile(&cpu, 0x1100, &[0x40], 1).await;
    assert!(cpu.install_code(&watched, 0).await.unwrap());
    let code = compile(&cpu, 0x100, &bytes, 10).await;
    assert!(cpu.install_code(&code, 0).await.unwrap());
    cpu.cpu_state().set_sp(0x2002);
    cpu.run(1).await;
    assert_eq!(cpu.cpu_state().sp(), 0x1ff2);
    assert_eq!(
        writes.get(),
        1,
        "the first stack transfer must not be replayed"
    );
    assert_eq!(cpu.memory().view().read_u16(0x1ff8).unwrap(), 0x2002);
}

async fn overlapping_enter_preserves_progress() {
    let mut cpu = create_cpu(Default::default()).await;
    let bytes = [0xc8, 0, 0, 5, 0xeb, 0xfa]; // ENTER 0,5; JMP 0100
    let memory = cpu.memory();
    memory.view().write(0x100, &bytes).unwrap();
    memory.view().write(0x1100, &[0x40, 0xeb, 0]).unwrap();
    for (address, value) in [
        (0x2002, 0x1111),
        (0x2000, 0x2222),
        (0x1ffe, 0x3333),
        (0x1ffc, 0x4444),
    ] {
        memory.view().write_u16(address, value).unwrap();
    }
    let watched = compile(&cpu, 0x1100, &[0x40], 1).await;
    assert!(cpu.install_code(&watched, 0).await.unwrap());
    let code = compile(&cpu, 0x100, &bytes, 1).await;
    assert!(cpu.install_code(&code, 0).await.unwrap());
    cpu.cpu_state().set_sp(0x2008);
    cpu.cpu_state().set_bp(0x2004);
    cpu.run(1).await;
    assert_eq!(
        cpu.cpu_state().eip(),
        0x100,
        "ENTER hands off before the first push"
    );
    assert_eq!(cpu.cpu_state().sp(), 0x1ffc);
    assert_eq!(cpu.cpu_state().bp(), 0x2006);
    assert_eq!(
        memory.view().read_u16(0x2004).unwrap(),
        0x1111,
        "replay would copy overwritten source 2222"
    );
    assert_eq!(memory.view().read_u16(0x1ffc).unwrap(), 0x2006);
}

async fn hooks_remain_boundaries() {
    let mut cpu = create_cpu(Default::default()).await;
    let bytes = [0x40, 0x43, 0xeb, 0xfc];
    cpu.memory().view().write(0x100, &bytes).unwrap();
    let code = compile(&cpu, 0x100, &bytes, 2).await;
    assert!(cpu.install_code(&code, 0).await.unwrap());
    let exit = cpu.exit_handle();
    cpu.install([Hook::host(0x101, async move || exit.request())])
        .await
        .unwrap();
    cpu.run(1).await;
    assert_eq!(
        cpu.cpu_state().eip(),
        0x100,
        "hook installation retired the compiled prefix"
    );
    assert!(
        !cpu.install_code(&code, 0).await.unwrap(),
        "compiled span crosses a hook"
    );
    // Interpreter blocks can also pass a straight-line hook, so enter it directly.
    cpu.cpu_state().set_eip(0x101);
    assert_eq!(cpu.run(10).await, ExecutionExit::Host(HostExit::Return));
    cpu.memory().view().write_u8(0x100, 0x43).unwrap();
    assert_eq!(
        cpu.run(10).await,
        ExecutionExit::Host(HostExit::Return),
        "invalidation preserves hook"
    );
}

struct CodeWriter(Memory);
impl Device for CodeWriter {
    fn write_port(&mut self, _: u16, _: u8, _: u32) {
        self.0.view().write_u8(0x8110, 0x43).unwrap();
    }
}

async fn device_writes_invalidate_before_dispatch() {
    let mut cpu = create_cpu(Default::default()).await;
    let memory = cpu.memory();
    let io = IoBus::new([DeviceConnection {
        device: Rc::new(RefCell::new(CodeWriter(memory.clone()))),
        ports: std::iter::once(0x10..=0x10).collect(),
        mmio: vec![],
    }]);
    cpu = Cpu::new(
        memory.clone(),
        CpuConfig::new(
            Compiler::new(ExecutionProfile::Real16).target(),
            CpuState {
                eip: 0x100,
                segments: Segments::real_mode(),
                ..Default::default()
            },
        )
        .unwrap(),
        io,
    )
    .await
    .unwrap();
    memory
        .view()
        .write(0x100, &[0xe6, 0x10, 0xeb, 0x0c])
        .unwrap();
    memory.view().write(0x110, &[0x40, 0xeb, 0]).unwrap();
    let code = compile(&cpu, 0x110, &[0x40], 1).await;
    assert!(cpu.install_code(&code, 0).await.unwrap());
    cpu.run(3).await;
    assert_eq!(cpu.cpu_state().ax(), 0);
    assert_eq!(cpu.cpu_state().bx(), 1);
    assert_eq!(cpu.cpu_state().eip(), 0x113);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn managed_code_admission_and_invalidation() {
    pollster::block_on(async {
        admission_and_host_writes().await;
        self_modification_restarts_before_effects().await;
        overlapping_enter_preserves_progress().await;
        hooks_remain_boundaries().await;
        device_writes_invalidate_before_dispatch().await;
    });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn managed_code_admission_and_invalidation() {
    admission_and_host_writes().await;
    self_modification_restarts_before_effects().await;
    overlapping_enter_preserves_progress().await;
    hooks_remain_boundaries().await;
    device_writes_invalidate_before_dispatch().await;
}
