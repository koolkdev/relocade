use super::*;

async fn captured_pages(compiler: &relocade_machine::CompilerService) {
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x2000,
            },
        ),
        (
            0x1000..=0x1fff,
            PhysicalMapping::Ram {
                backing_offset: 0x2000,
            },
        ),
    ])
    .unwrap();
    let mut machine = Machine::new(MachineConfig::new(0x3000, map).unwrap()).unwrap();
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
    )
    .unwrap();
    let memory = machine.memory();
    memory.view().write(0x100, &[0xa3, 0, 0x12]).unwrap(); // MOV [1200],AX through an alias.
    memory.view().write(0x300, &[0xc3]).unwrap(); // Captured code shares the backing page.
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let mut late_handle = Some(machine.create_cpu(config, None).unwrap());
    cpu.run_until(pending::<()>(), async move |mut runner| {
        let target = runner.cpu().cpu_state().execution_target();
        let mut compiler = runner.take_compiler().unwrap();
        for late_cpu in [false, true] {
            compiler
                .request(
                    target,
                    vec![BlockRequest {
                        address: 0x300,
                        start_eip: 0x300,
                        byte_len: 1,
                        instruction_limit: 1,
                    }],
                )
                .unwrap();
            let code = compiler.recv().await.unwrap().unwrap();
            assert!(!code.source().has_recorded_writes());
            let memory = runner.cpu().memory();
            let mut late = if late_cpu {
                Some(
                    late_handle
                        .take()
                        .unwrap()
                        .into_cpu(Default::default())
                        .await
                        .unwrap(),
                )
            } else {
                None
            };
            let cpu = late.as_mut().unwrap_or_else(|| runner.cpu());
            cpu.cpu_state().set_eip(0x100);
            cpu.cpu_state()
                .set_ax(if late_cpu { 0x5678 } else { 0x1234 });
            assert_eq!(cpu.step(), ExecutionExit::StepComplete);
            assert_eq!(
                memory.view().read_u16(0x200).unwrap(),
                if late_cpu { 0x5678 } else { 0x1234 }
            );
            assert!(
                code.source().has_recorded_writes(),
                "guest writes reach the shared registry"
            );
        }
    })
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn captured_pages_intercept_writes_from_existing_and_later_cpus() {
    pollster::block_on(compilation::run(captured_pages));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn captured_pages_intercept_writes_from_existing_and_later_cpus() {
    compilation::run(captured_pages).await;
}
