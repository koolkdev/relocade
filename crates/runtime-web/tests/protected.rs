use relocade_machine::{MachineConfig, PagedMemoryMap};
use relocade_runtime::{DispatchEntry, Runtime, RuntimeState};
use relocade_x86::{
    Compiler, CpuState, Descriptor, DescriptorTables, Exception, ExecutionExit, Segment,
    SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind, SegmentLimit, SegmentProfile,
    StoredTableRegister,
};

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn data(base: u32) -> SegmentDescriptor {
    SegmentDescriptor::new(
        base,
        SegmentLimit::bytes(0xffff).unwrap(),
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        SegmentDefaultSize::Bits32,
    )
}

async fn runtime() -> Runtime<Backend<RuntimeState>> {
    let mut map = PagedMemoryMap::default();
    map.map(0..=0xfff, 0, true).unwrap();
    map.map(0x400000..=0x400fff, 0x1000, true).unwrap();
    map.map(0x401000..=0x401fff, 0x2000, false).unwrap();
    map.map(0x500000..=0x500fff, 0x3000, true).unwrap();
    let mut tables = DescriptorTables::default();
    tables.insert(0x23, data(0x400000));
    tables.insert(0x0f, data(0x500000));
    let config = MachineConfig::new(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState::default(),
        0x5000,
        map,
    )
    .unwrap()
    .with_descriptor_tables(tables, Some(0x28))
    .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let backend = Backend::with_state(RuntimeState::default()).unwrap();
    #[cfg(target_arch = "wasm32")]
    let backend = Backend::with_state(RuntimeState::default());
    Runtime::new(backend, config).await.unwrap()
}

async fn execute(
    runtime: &mut Runtime<Backend<RuntimeState>>,
    code: &[u8],
    jit: bool,
) -> ExecutionExit {
    let mut bytes = code.to_vec();
    bytes.extend_from_slice(&[0xeb, 0]); // End the interpreter block after the operation.
    runtime.machine().memory().write(0x100, &bytes).unwrap();
    if jit {
        let module = Compiler::new(SegmentProfile::Flat32)
            .compile_block(0x100, code, 1)
            .unwrap();
        runtime
            .install_module(
                &module.bytes,
                &[DispatchEntry {
                    address: 0x100,
                    export: &module.entry,
                }],
            )
            .await
            .unwrap();
    }
    runtime.machine().cpu().set_eip(0x100);
    runtime.run(1).unwrap()
}

async fn check_descriptor_runtime() {
    for jit in [false, true] {
        let mut runtime = runtime().await;
        runtime
            .machine()
            .memory()
            .write(0x400000, &0x11223344u32.to_le_bytes())
            .unwrap();
        runtime
            .machine()
            .memory()
            .write(0x500000, &0x55667788u32.to_le_bytes())
            .unwrap();
        let mut bytes = Descriptor::from(data(0x400000)).to_bytes();
        bytes[5] &= !1;
        runtime
            .machine()
            .descriptors()
            .write(0x23, Descriptor::from_bytes(bytes))
            .unwrap();
        runtime.machine().cpu().set_ax(0x23);
        assert_eq!(
            execute(&mut runtime, &[0x8e, 0xe0], jit).await,
            ExecutionExit::BudgetExhausted
        ); // MOV FS,AX
        let fs = runtime.machine().cpu().segment(Segment::Fs);
        assert_eq!((fs.selector, fs.base), (0x23, 0x400000));
        assert_eq!(
            runtime
                .machine()
                .descriptors()
                .read(0x23)
                .unwrap()
                .to_bytes()[5],
            0xf3
        );
        runtime
            .machine()
            .descriptors()
            .write(0x23, data(0x500000))
            .unwrap();
        assert_eq!(runtime.machine().cpu().segment(Segment::Fs), fs);
        assert_eq!(
            execute(&mut runtime, &[0x64, 0xa1, 0, 0, 0, 0], jit).await,
            ExecutionExit::BudgetExhausted
        );
        assert_eq!(runtime.machine().cpu().eax(), 0x11223344); // Loaded FS retains the old base.
        runtime.machine().cpu().set_ax(0x23);
        execute(&mut runtime, &[0x8e, 0xe0], jit).await;
        execute(&mut runtime, &[0x64, 0xa1, 0, 0, 0, 0], jit).await;
        assert_eq!(runtime.machine().cpu().eax(), 0x55667788);
        runtime.machine().cpu().set_ax(0x0f);
        execute(&mut runtime, &[0x8e, 0xe0], jit).await;
        assert_eq!(runtime.machine().cpu().segment(Segment::Fs).selector, 0x0f);
        assert_eq!(runtime.machine().cpu().segment(Segment::Fs).base, 0x500000);

        runtime.machine().cpu().set_ax(0x23);
        execute(&mut runtime, &[0x0f, 0x02, 0xc8], jit).await; // LAR ECX,AX
        assert_eq!(runtime.machine().cpu().ecx(), 0x0040f300);
        assert_ne!(runtime.machine().cpu().eflags() & 0x40, 0);
        execute(&mut runtime, &[0x0f, 0x03, 0xd0], jit).await; // LSL EDX,AX
        assert_eq!(runtime.machine().cpu().edx(), 0xffff);

        runtime
            .machine()
            .descriptors()
            .write(0x33, Descriptor::from_bytes([0, 0, 0, 0, 0, 0xec, 0, 0]))
            .unwrap(); // 32-bit call gate
        runtime.machine().cpu().set_ax(0x33);
        execute(&mut runtime, &[0x0f, 0x02, 0xc8], jit).await;
        assert_eq!(runtime.machine().cpu().ecx(), 0xec00);
        assert_ne!(runtime.machine().cpu().eflags() & 0x40, 0);
        runtime.machine().cpu().set_edx(0xdeadbeef);
        execute(&mut runtime, &[0x0f, 0x03, 0xd0], jit).await;
        assert_eq!(runtime.machine().cpu().edx(), 0xdeadbeef);
        assert_eq!(runtime.machine().cpu().eflags() & 0x40, 0);

        runtime
            .machine()
            .cpu()
            .set_gdtr(StoredTableRegister::new(0, 0x26));
        runtime.machine().cpu().set_ax(0x23);
        runtime.machine().cpu().set_eip(0x100);
        let before = runtime.machine().cpu().snapshot();
        assert_eq!(
            execute(&mut runtime, &[0x8e, 0xe0], jit).await,
            ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0x20 })
        );
        assert_eq!(runtime.machine().cpu().snapshot(), before);
    }
}

async fn check_page_faults_and_accessed_commit() {
    for jit in [false, true] {
        let mut runtime = runtime().await;
        runtime
            .machine()
            .memory()
            .write_backing(0x1ffe, b"AB")
            .unwrap();
        runtime
            .machine()
            .memory()
            .write_backing(0x2000, b"CD")
            .unwrap();
        runtime.machine().cpu().set_eax(0x44332211);
        runtime.machine().cpu().set_eip(0x100);
        let before = runtime.machine().cpu().snapshot();
        assert_eq!(
            execute(&mut runtime, &[0xa3, 0xfe, 0x0f, 0x40, 0], jit).await,
            ExecutionExit::Exception(Exception::PageFault {
                linear_address: 0x401000,
                error_code: 3
            })
        );
        assert_eq!(runtime.machine().cpu().snapshot(), before);
        let mut bytes = [0; 4];
        runtime
            .machine()
            .memory()
            .read(0x400ffe, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"ABCD");
        runtime
            .machine()
            .memory()
            .map_pages(0x401000..=0x401fff, 0x4000, true)
            .unwrap();
        assert_eq!(
            execute(&mut runtime, &[0xa3, 0xfe, 0x0f, 0x40, 0], jit).await,
            ExecutionExit::BudgetExhausted
        );
        runtime
            .machine()
            .memory()
            .read(0x400ffe, &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0x11, 0x22, 0x33, 0x44]);

        let code = Descriptor::from_bytes([0xff, 0, 0, 0, 0, 0xfa, 0x40, 0]);
        runtime.machine().descriptors().write(0x3b, code).unwrap();
        runtime.machine().cpu().set_eip(0x100);
        let before = runtime.machine().cpu().snapshot();
        assert_eq!(
            execute(&mut runtime, &[0xea, 0, 2, 0, 0, 0x3b, 0], jit).await,
            ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 })
        );
        assert_eq!(runtime.machine().cpu().snapshot(), before);
        assert_eq!(runtime.machine().descriptors().read(0x3b), Some(code));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn protected_descriptors_and_cached_segments_share_one_live_machine() {
    pollster::block_on(check_descriptor_runtime());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn protected_descriptors_and_cached_segments_share_one_live_machine() {
    check_descriptor_runtime().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn protected_faults_preserve_instruction_state_and_defer_accessed_updates() {
    pollster::block_on(check_page_faults_and_accessed_commit());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn protected_faults_preserve_instruction_state_and_defer_accessed_updates() {
    check_page_faults_and_accessed_commit().await;
}

async fn check_profile_admission() {
    for jit in [false, true] {
        let mut runtime = runtime().await;
        runtime.machine().cpu().set_ax(0x23);
        assert_eq!(
            execute(&mut runtime, &[0x8e, 0xd8], jit).await,
            ExecutionExit::BudgetExhausted
        ); // MOV DS,AX
        let completed = runtime.machine().cpu().snapshot();
        assert_eq!(completed.segments.ds.base, 0x400000);
        assert_eq!(completed.eip, 0x102);
        assert_eq!(completed.instruction_count, 1);
        assert_eq!(runtime.run(1).unwrap(), ExecutionExit::ProfileChanged);
        assert_eq!(runtime.machine().cpu().snapshot(), completed);
        runtime
            .machine()
            .cpu()
            .set_segment(Segment::Ds, relocade_x86::StoredSegment::flat_data32(0x23))
            .unwrap();
        runtime
            .machine()
            .descriptors()
            .write(
                0x3b,
                Descriptor::from_bytes([0xff, 0xff, 0, 0, 0, 0xfa, 0x8f, 0]),
            )
            .unwrap();
        assert_eq!(
            execute(&mut runtime, &[0xea, 0, 2, 0, 0, 0x3b, 0], jit).await,
            ExecutionExit::BudgetExhausted
        );
        let completed = runtime.machine().cpu().snapshot();
        assert_eq!((completed.segments.cs.big, completed.eip), (0, 0x200));
        assert_eq!(
            runtime
                .machine()
                .descriptors()
                .read(0x3b)
                .unwrap()
                .to_bytes()[5],
            0xfb
        );
        assert_eq!(runtime.run(1).unwrap(), ExecutionExit::ProfileChanged);
        assert_eq!(runtime.machine().cpu().snapshot(), completed);
    }
    // Direct interpreter fallback is a separate exported gateway.
    let mut runtime = runtime().await;
    let mut program = relocade_compiler::Program::new();
    let context = program.import_memory(relocade_x86::abi::EXECUTION_CONTEXT);
    let interpret = program.import_function(relocade_x86::abi::INTERPRET);
    let entry = program
        .function(relocade_x86::abi::RUN.signature(), |mut body| {
            body.store(
                context,
                std::mem::offset_of!(relocade_x86::ExecutionContext, cpu.segments.ds.base) as u32,
                relocade_compiler::Val::<relocade_compiler::I32>::from(0x1000),
            )?;
            body.tail_call(interpret, &[])
        })
        .unwrap();
    relocade_x86::abi::RUN.export(&mut program, entry).unwrap();
    runtime
        .install_module(
            &program.compile().unwrap(),
            &[DispatchEntry {
                address: 0x300,
                export: "run",
            }],
        )
        .await
        .unwrap();
    runtime.machine().cpu().set_eip(0x300);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::ProfileChanged);
    assert_eq!(runtime.machine().cpu().eip(), 0x300);
    assert_eq!(runtime.machine().cpu().instruction_count(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn profile_changes_stop_before_incompatible_dispatch_or_interpreter_entry() {
    pollster::block_on(check_profile_admission());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn profile_changes_stop_before_incompatible_dispatch_or_interpreter_entry() {
    check_profile_admission().await;
}
