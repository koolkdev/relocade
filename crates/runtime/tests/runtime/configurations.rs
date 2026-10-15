use super::*;
use wasm86_machine::{MemoryError, PagedMemoryMap};
use wasm86_runtime::ExecutableModule;
use wasm86_x86::{
    Compiler, CpuFeature, CpuModel, CpuState, DescriptorTables, Exception, ExecutionExit,
    ExecutionTarget, Segment, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorInfo,
    SegmentDescriptorKind, SegmentLimit, SegmentProfile, StoredSegment,
};

#[derive(Default)]
struct Descriptors {
    tables: DescriptorTables,
    loads: u32,
    queries: u32,
}
impl Environment for Descriptors {
    fn resolve_segment(
        &mut self,
        segment: Segment,
        selector: u16,
    ) -> Result<StoredSegment, Exception> {
        self.loads += 1;
        self.tables.resolve_user_segment(segment, selector)
    }
    fn query_segment_descriptor(&mut self, selector: u16) -> SegmentDescriptorInfo {
        self.queries += 1;
        self.tables.query_user_segment_descriptor(selector)
    }
}
fn descriptor(base: u32) -> SegmentDescriptor {
    SegmentDescriptor::new(
        base,
        SegmentLimit::bytes(0xfff).unwrap(),
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        SegmentDefaultSize::Bits32,
    )
}
fn flat_machine<E: Environment>(code: &[u8], environment: E) -> Machine<E> {
    flat_machine_for(
        code,
        environment,
        Compiler::new(SegmentProfile::Flat32).target(),
    )
}
fn flat_machine_for<E: Environment>(
    code: &[u8],
    environment: E,
    target: ExecutionTarget,
) -> Machine<E> {
    let mut map = PagedMemoryMap::default();
    map.map(0x1000..=0x1fff, 0, true).unwrap();
    map.map(0x80001000..=0x80001fff, 0x2000, true).unwrap();
    let cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    let context = CpuContext::new(target, cpu).unwrap();
    let mut machine = Machine::new(context, map, environment).unwrap();
    machine.initialize_backing(0, code).unwrap();
    machine
        .initialize_backing(0x2000, &42u32.to_le_bytes())
        .unwrap();
    machine
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn one_runtime_executes_real_and_flat_machines_with_private_backing() {
    let runtime = runtime().await;
    let mut real = real_machine(ram(), ());
    real.initialize_backing(0, &[0xb8, 7, 0, 0xeb, 0]).unwrap();
    let mut real = runtime.attach(real).await.unwrap();
    // MOV EAX, [80001000h] accesses high linear addresses through remapped backing.
    let mut flat = runtime
        .attach(flat_machine(&[0xa1, 0, 0x10, 0, 0x80, 0xeb, 0], ()))
        .await
        .unwrap();
    assert_eq!(flat.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(flat.cpu().registers.eax, 42);
    assert_eq!(real.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(real.cpu().registers.eax, 7);
    assert_eq!(flat.cpu().eip, 0x1007);
    assert_eq!(real.cpu().eip, 5);
    flat.write_memory(0x80001000, &[9]).unwrap();
    let mut byte = [0];
    real.read_memory(0, &mut byte).unwrap();
    assert_eq!(byte, [0xb8]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn protected_descriptor_callbacks_load_fs_and_query_access_rights() {
    let mut environment = Descriptors::default();
    environment.tables.insert(0x23, descriptor(0x80001000));
    // MOV AX,23h; MOV FS,AX; LAR EDX,AX; MOV EAX,FS:[0]; UD2
    let code = [
        0x66, 0xb8, 0x23, 0, 0x8e, 0xe0, 0x0f, 0x02, 0xd0, 0x64, 0xa1, 0, 0, 0, 0, 0x0f, 0x0b,
    ];
    let mut session = runtime()
        .await
        .attach(flat_machine(&code, environment))
        .await
        .unwrap();
    let outcome = session.run(20).unwrap();
    assert_eq!(session.cpu().instruction_count, 4);
    assert_eq!(outcome, ExecutionExit::Exception(Exception::InvalidOpcode));
    assert_eq!(session.cpu().registers.eax, 42);
    assert_eq!(session.cpu().registers.edx & 0xff00, 0xf300);
    assert_eq!(session.cpu().segments.fs.base, 0x80001000);
    assert_eq!(
        session.with_environment(|env| (env.loads, env.queries)),
        (1, 1)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn nonflat_segment_load_stops_before_another_callable_runs() {
    let mut environment = Descriptors::default();
    environment.tables.insert(0x23, descriptor(0x80001000));
    // MOV AX,23h; MOV DS,AX; INC EAX. DS loading is valid but ends Flat32 execution.
    let mut session = runtime()
        .await
        .attach(flat_machine(
            &[0x66, 0xb8, 0x23, 0, 0x8e, 0xd8, 0x40],
            environment,
        ))
        .await
        .unwrap();
    let code = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1006, &[0x40], 1)
        .unwrap();
    let export = code.entry.clone();
    let mut module = ExecutableModule::new(code);
    module.bind_entry(0x1006, export);
    session.install_module(module).await.unwrap();
    let outcome = session.run(10).unwrap();
    assert_eq!(session.cpu().instruction_count, 2);
    assert_eq!(outcome, ExecutionExit::ContextChanged);
    assert_eq!(session.cpu().eip, 0x1006);
    assert_eq!(session.cpu().registers.eax, 0x23);
    assert_eq!(session.cpu().segments.ds.base, 0x80001000);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn paged_permissions_and_top_address_are_preserved_in_both_access_paths() {
    let mut session = runtime()
        .await
        .attach(flat_machine(&[0xa1, 0, 0x10, 0, 0x80, 0xeb, 0], ()))
        .await
        .unwrap();
    let mut map = PagedMemoryMap::default();
    map.map(0x1000..=0x1fff, 0, true).unwrap();
    map.map(0x80001000..=0x80001fff, 0x2000, false).unwrap();
    map.map(0xfffff000..=u32::MAX, 0x3000, true).unwrap();
    session.set_memory_map(map).unwrap();
    assert_eq!(
        session.write_memory(0x80001000, &[1]),
        Err(MemoryError::ReadOnly {
            address: 0x80001000
        })
    );
    session.write_memory(u32::MAX, &[9]).unwrap();
    let mut byte = [0];
    session.read_memory(u32::MAX, &mut byte).unwrap();
    assert_eq!(byte, [9]);
    assert_eq!(session.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(session.cpu().registers.eax, 42);
    session
        .write_memory(0x1007, &[0xa3, 0, 0x10, 0, 0x80])
        .unwrap();
    assert_eq!(
        session.run(1).unwrap(),
        ExecutionExit::Exception(Exception::PageFault {
            linear_address: 0x80001000,
            error_code: 3
        })
    );
    assert_eq!(session.cpu().eip, 0x1007);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn prepared_code_and_installed_modules_preserve_cpu_capabilities() {
    let runtime = runtime().await;
    let code = [0x0f, 0x44, 0xc0, 0xeb, 0]; // CMOVE EAX,EAX; JMP to the next block
    let mut enabled = runtime.attach(flat_machine(&code, ())).await.unwrap();
    let compiler = Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false));
    let mut disabled = runtime
        .attach(flat_machine_for(&code, (), compiler.target()))
        .await
        .unwrap();
    assert_eq!(enabled.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(
        disabled.run(1).unwrap(),
        ExecutionExit::Exception(Exception::InvalidOpcode)
    );
    assert_eq!(disabled.cpu().eip, 0x1000);
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &code, 1)
        .unwrap();
    assert!(matches!(
        disabled
            .install_module(ExecutableModule::new(compiled))
            .await,
        Err(wasm86_runtime::Error::IncompatibleModule)
    ));
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn rejected_descriptor_load_preserves_the_destination_segment() {
    // MOV AX,23h; MOV DS,AX with no matching descriptor.
    let mut session = runtime()
        .await
        .attach(flat_machine(
            &[0x66, 0xb8, 0x23, 0, 0x8e, 0xd8],
            Descriptors::default(),
        ))
        .await
        .unwrap();
    let ds = session.cpu().segments.ds;
    let outcome = session.run(10).unwrap();
    assert_eq!(session.cpu().instruction_count, 1);
    assert_eq!(
        outcome,
        ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0x20 })
    );
    assert_eq!(session.cpu().segments.ds, ds);
    assert_eq!(session.cpu().eip, 0x1004);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn configured_host_functions_use_the_target_and_return_through_a_32_bit_stack() {
    use wasm86_machine::{HostAction, HostReturn};
    #[derive(Default)]
    struct Calls(u32);
    impl Environment for Calls {}
    let target = Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false))
        .target();
    // PUSH 11112222h; CALL 2000h; UD2. The service address has no guest mapping.
    let mut machine = flat_machine_for(
        &[
            0x68, 0x22, 0x22, 0x11, 0x11, 0xe8, 0xf6, 0x0f, 0, 0, 0x0f, 0x0b,
        ],
        Calls::default(),
        target,
    );
    let mut cpu = machine.context().cpu();
    cpu.registers.esp = 0x80002000;
    cpu.instruction_count = 100;
    machine.set_cpu(cpu).unwrap();
    machine
        .host_functions_mut()
        .insert(0x2000, HostReturn::Near { pop_bytes: 4 }, |host| {
            let before = host.cpu();
            assert_eq!(before.instruction_count, 102);
            let mut frame = [0; 8];
            host.read_memory(before.registers.esp, &mut frame).unwrap();
            assert_eq!(frame, [0x0a, 0x10, 0, 0, 0x22, 0x22, 0x11, 0x11]);
            let mut invalid = before;
            invalid.segments.ds.base = 1;
            assert!(host.set_cpu(invalid).is_err());
            assert_eq!(host.cpu(), before);
            let mut result = before;
            result.registers.eax = 42;
            result.instruction_count = 0; // callback edits cannot manufacture retirement
            host.set_cpu(result).unwrap();
            host.with_environment(|calls| calls.0 += 1);
            HostAction::Continue
        });
    let mut session = runtime().await.attach(machine).await.unwrap();
    let outcome = session.run(20).unwrap();
    assert_eq!(outcome, ExecutionExit::Exception(Exception::InvalidOpcode));
    let cpu = session.cpu();
    assert_eq!(
        (
            cpu.registers.eax,
            cpu.registers.esp,
            cpu.eip,
            cpu.instruction_count
        ),
        (42, 0x80002000, 0x100a, 103)
    );
    assert_eq!(session.with_environment(|calls| calls.0), 1);
}
