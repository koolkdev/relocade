mod support;
use relocade_machine::{Cpu, DispatchEntry, RuntimeState};
use relocade_machine::{CpuConfig, PagedMemoryMap};
use relocade_x86::{
    Compiler, CpuState, Descriptor, DescriptorTableRegister, DescriptorTables, Exception,
    ExecutionExit, Segment, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind,
    SegmentLimit, SegmentProfile,
};

use relocade_wasm_runtime::Backend;
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

async fn runtime(profile: SegmentProfile) -> Cpu<Backend<RuntimeState>> {
    let mut map = PagedMemoryMap::default();
    map.map(0..=0xfff, 0, true).unwrap();
    map.map(0x400000..=0x400fff, 0x1000, true).unwrap();
    map.map(0x401000..=0x401fff, 0x2000, false).unwrap();
    map.map(0x500000..=0x500fff, 0x3000, true).unwrap();
    let mut tables = DescriptorTables::default();
    tables.insert(0x23, data(0x400000));
    tables.insert(0x0f, data(0x500000));
    let config = support::with_descriptors(
        support::configuration(
            Compiler::new(profile).target(),
            CpuState::default(),
            0x5000,
            map,
        )
        .unwrap(),
        tables,
        Some(0x28),
    )
    .unwrap();
    let backend = Backend::with_state(RuntimeState::default()).unwrap();
    support::create_cpu(backend, config).await.unwrap()
}

async fn execute(
    runtime: &mut Cpu<Backend<RuntimeState>>,
    code: &[u8],
    jit: bool,
) -> ExecutionExit {
    let mut bytes = code.to_vec();
    bytes.extend_from_slice(&[0xeb, 0]); // End the interpreter block after the operation.
    runtime.context().memory().write(0x100, &bytes).unwrap();
    if jit {
        let mut source = vec![0; code.len()];
        let version = runtime.memory().snapshot(0x100, &mut source).unwrap();
        let module = Compiler::new(runtime.context().execution_target().profile)
            .compile_block(0x100, &source, 1)
            .unwrap();
        runtime
            .install_module(
                &version,
                &module.bytes,
                &[DispatchEntry {
                    address: 0x100,
                    export: &module.entry,
                }],
            )
            .await
            .unwrap();
    }
    runtime.context().cpu().set_eip(0x100);
    runtime.run(1).unwrap()
}

async fn check_descriptor_runtime() {
    for jit in [false, true] {
        let mut runtime = runtime(SegmentProfile::Flat32).await;
        runtime
            .context()
            .descriptors()
            .write(
                0x43,
                SegmentDescriptor {
                    limit: SegmentLimit::pages(0xfffff).unwrap(),
                    ..data(0)
                },
            )
            .unwrap();
        runtime.context().cpu().set_ax(0x43);
        assert_eq!(
            execute(&mut runtime, &[0x8e, 0xd8], jit).await,
            ExecutionExit::BudgetExhausted
        ); // MOV DS,AX: a different selector with compatible flat assumptions.
        assert_eq!(runtime.context().cpu().segment(Segment::Ds).selector, 0x43);
        runtime
            .context()
            .memory()
            .write(0x400000, &0x11223344u32.to_le_bytes())
            .unwrap();
        runtime
            .context()
            .memory()
            .write(0x500000, &0x55667788u32.to_le_bytes())
            .unwrap();
        let mut bytes = Descriptor::from(data(0x400000)).to_bytes();
        bytes[5] &= !1;
        runtime
            .context()
            .descriptors()
            .write(0x23, Descriptor::from_bytes(bytes))
            .unwrap();
        runtime.context().cpu().set_ax(0x23);
        assert_eq!(
            execute(&mut runtime, &[0x8e, 0xe0], jit).await,
            ExecutionExit::BudgetExhausted
        ); // MOV FS,AX
        let fs = runtime.context().cpu().segment(Segment::Fs);
        assert_eq!((fs.selector, fs.base), (0x23, 0x400000));
        assert_eq!(
            runtime
                .context()
                .descriptors()
                .read(0x23)
                .unwrap()
                .to_bytes()[5],
            0xf3
        );
        runtime
            .context()
            .descriptors()
            .write(0x23, data(0x500000))
            .unwrap();
        assert_eq!(runtime.context().cpu().segment(Segment::Fs), fs);
        assert_eq!(
            execute(&mut runtime, &[0x64, 0xa1, 0, 0, 0, 0], jit).await,
            ExecutionExit::BudgetExhausted
        );
        assert_eq!(runtime.context().cpu().eax(), 0x11223344); // Loaded FS retains the old base.
        runtime.context().cpu().set_ax(0x23);
        execute(&mut runtime, &[0x8e, 0xe0], jit).await;
        execute(&mut runtime, &[0x64, 0xa1, 0, 0, 0, 0], jit).await;
        assert_eq!(runtime.context().cpu().eax(), 0x55667788);
        let mut local = Descriptor::from(data(0x500000)).to_bytes();
        local[5] &= !1;
        runtime
            .context()
            .descriptors()
            .write(0x0f, Descriptor::from_bytes(local))
            .unwrap();
        runtime.context().cpu().set_ax(0x0f);
        execute(&mut runtime, &[0x8e, 0xe0], jit).await;
        assert_eq!(runtime.context().cpu().segment(Segment::Fs).selector, 0x0f);
        assert_eq!(runtime.context().cpu().segment(Segment::Fs).base, 0x500000);
        local[5] |= 1;
        assert_eq!(
            runtime
                .context()
                .descriptors()
                .read(0x0f)
                .unwrap()
                .to_bytes(),
            local
        );

        runtime.context().cpu().set_ax(0x23);
        execute(&mut runtime, &[0x0f, 0x02, 0xc8], jit).await; // LAR ECX,AX
        assert_eq!(runtime.context().cpu().ecx(), 0x0040f300);
        assert_ne!(runtime.context().cpu().eflags() & 0x40, 0);
        execute(&mut runtime, &[0x0f, 0x03, 0xd0], jit).await; // LSL EDX,AX
        assert_eq!(runtime.context().cpu().edx(), 0xffff);

        runtime
            .context()
            .descriptors()
            .write(0x33, Descriptor::from_bytes([0, 0, 0, 0, 0, 0xec, 0, 0]))
            .unwrap(); // 32-bit call gate
        runtime.context().cpu().set_ax(0x33);
        execute(&mut runtime, &[0x0f, 0x02, 0xc8], jit).await;
        assert_eq!(runtime.context().cpu().ecx(), 0xec00);
        assert_ne!(runtime.context().cpu().eflags() & 0x40, 0);
        runtime.context().cpu().set_edx(0xdeadbeef);
        execute(&mut runtime, &[0x0f, 0x03, 0xd0], jit).await;
        assert_eq!(runtime.context().cpu().edx(), 0xdeadbeef);
        assert_eq!(runtime.context().cpu().eflags() & 0x40, 0);

        runtime
            .context()
            .cpu()
            .set_gdtr(DescriptorTableRegister::new(0, 0x26));
        runtime.context().cpu().set_ax(0x23);
        runtime.context().cpu().set_eip(0x100);
        let before = runtime.context().cpu().snapshot();
        assert_eq!(
            execute(&mut runtime, &[0x8e, 0xe0], jit).await,
            ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0x20 })
        );
        assert_eq!(runtime.context().cpu().snapshot(), before);
    }
}

async fn check_faults_and_segment_load_completion() {
    for jit in [false, true] {
        let mut runtime = runtime(SegmentProfile::Segmented32).await;
        runtime
            .context()
            .memory()
            .write_backing(0x1ffe, b"AB")
            .unwrap();
        runtime
            .context()
            .memory()
            .write_backing(0x2000, b"CD")
            .unwrap();
        runtime.context().cpu().set_eax(0x44332211);
        runtime.context().cpu().set_eip(0x100);
        let before = runtime.context().cpu().snapshot();
        assert_eq!(
            execute(&mut runtime, &[0xa3, 0xfe, 0x0f, 0x40, 0], jit).await,
            ExecutionExit::Exception(Exception::PageFault {
                linear_address: 0x401000,
                error_code: 3
            })
        );
        assert_eq!(runtime.context().cpu().snapshot(), before);
        let mut bytes = [0; 4];
        runtime
            .context()
            .memory()
            .read(0x400ffe, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"ABCD");
        runtime
            .context()
            .memory()
            .map_pages(0x401000..=0x401fff, 0x4000, true)
            .unwrap();
        assert_eq!(
            execute(&mut runtime, &[0xa3, 0xfe, 0x0f, 0x40, 0], jit).await,
            ExecutionExit::BudgetExhausted
        );
        runtime
            .context()
            .memory()
            .read(0x400ffe, &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0x11, 0x22, 0x33, 0x44]);

        let code = Descriptor::from_bytes([0xff, 0, 0, 0, 0, 0xfa, 0x40, 0]);
        runtime.context().descriptors().write(0x3b, code).unwrap();
        runtime.context().cpu().set_eip(0x100);
        let before = runtime.context().cpu().snapshot();
        assert_eq!(
            execute(&mut runtime, &[0xea, 0, 2, 0, 0, 0x3b, 0], jit).await,
            ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0 })
        );
        assert_eq!(runtime.context().cpu().snapshot(), before);
        assert_eq!(runtime.context().descriptors().read(0x3b), Some(code));
        // A successful transfer normalizes CS.RPL to CPL, while completion
        // still addresses the same descriptor slot.
        assert_eq!(
            execute(&mut runtime, &[0xea, 0x80, 0, 0, 0, 0x38, 0], jit).await,
            ExecutionExit::BudgetExhausted
        );
        assert_eq!(runtime.context().cpu().segment(Segment::Cs).selector, 0x3b);
        assert_eq!(runtime.context().cpu().eip(), 0x80);
        let mut accessed = code.to_bytes();
        accessed[5] |= 1;
        assert_eq!(
            runtime
                .context()
                .descriptors()
                .read(0x3b)
                .unwrap()
                .to_bytes(),
            accessed
        );
    }
}

async fn check_shared_caches() {
    for jit in [false, true] {
        let mut first = runtime(SegmentProfile::Flat32).await;
        let cpu = CpuConfig::new(
            first.context().execution_target(),
            first.context().cpu().snapshot(),
        )
        .unwrap();
        let backend = Backend::with_state(RuntimeState::default()).unwrap();
        let mut second = first.machine().create_cpu(backend, cpu).await.unwrap();
        first
            .context()
            .memory()
            .write(0x400000, &0x11223344u32.to_le_bytes())
            .unwrap();
        first
            .context()
            .memory()
            .write(0x500000, &0x55667788u32.to_le_bytes())
            .unwrap();
        first.context().cpu().set_ax(0x23);
        assert_eq!(
            execute(&mut first, &[0x8e, 0xe0], jit).await,
            ExecutionExit::BudgetExhausted,
        ); // MOV FS,AX
        first
            .context()
            .descriptors()
            .write(0x23, data(0x500000))
            .unwrap();
        second.context().cpu().set_ax(0x23);
        assert_eq!(
            execute(&mut second, &[0x8e, 0xe0], jit).await,
            ExecutionExit::BudgetExhausted,
        );
        for (runtime, expected) in [(&mut first, 0x11223344), (&mut second, 0x55667788)] {
            assert_eq!(
                execute(runtime, &[0x64, 0xa1, 0, 0, 0, 0], jit).await,
                ExecutionExit::BudgetExhausted,
            ); // MOV EAX,FS:[0]
            assert_eq!(runtime.context().cpu().eax(), expected);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn two_cpus_execute_with_independent_caches_over_shared_descriptor_bytes() {
    pollster::block_on(check_shared_caches());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn two_cpus_execute_with_independent_caches_over_shared_descriptor_bytes() {
    check_shared_caches().await;
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
    pollster::block_on(check_faults_and_segment_load_completion());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn protected_faults_preserve_instruction_state_and_defer_accessed_updates() {
    check_faults_and_segment_load_completion().await;
}

// Browser Rust panics abort the host Wasm instance; test the panic contract natively.
#[cfg(not(target_arch = "wasm32"))]
async fn incompatible_segment_load(jit: bool, segment: Segment) {
    let mut runtime = runtime(SegmentProfile::Flat32).await;
    let code: &[u8] = match segment {
        Segment::Ds => {
            runtime.context().cpu().set_ax(0x23);
            &[0x8e, 0xd8] // MOV DS,AX: the descriptor has a nonzero base.
        }
        Segment::Cs => {
            runtime
                .context()
                .descriptors()
                .write(
                    0x3b,
                    Descriptor::from_bytes([0xff, 0xff, 0, 0, 0, 0xfa, 0x8f, 0]),
                )
                .unwrap();
            &[0xea, 0, 2, 0, 0, 0x3b, 0] // Far JMP to 16-bit code.
        }
        _ => unreachable!(),
    };
    execute(&mut runtime, code, jit).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "segment Ds violates execution profile Protected(Flat32)")]
fn interpreted_data_segment_load_panics_when_it_violates_the_profile() {
    pollster::block_on(incompatible_segment_load(false, Segment::Ds));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "segment Cs violates execution profile Protected(Flat32)")]
fn interpreted_code_segment_load_panics_when_it_violates_the_profile() {
    pollster::block_on(incompatible_segment_load(false, Segment::Cs));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "segment Ds violates execution profile Protected(Flat32)")]
fn compiled_data_segment_load_panics_when_it_violates_the_profile() {
    pollster::block_on(incompatible_segment_load(true, Segment::Ds));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "segment Cs violates execution profile Protected(Flat32)")]
fn compiled_code_segment_load_panics_when_it_violates_the_profile() {
    pollster::block_on(incompatible_segment_load(true, Segment::Cs));
}
