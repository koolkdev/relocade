use super::*;
use futures_channel::oneshot;
use std::time::Duration;

async fn register_while_writing(compiler: &relocade_machine::CompilerService) {
    let ram = PhysicalMapping::Ram { backing_offset: 0 };
    let map = PhysicalMemoryMap::new([(0..=0xfff, ram), (0x1000..=0x1fff, ram)]).unwrap();
    let mut machine = Machine::new(MachineConfig::new(0x1000, map).unwrap()).unwrap();
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
    )
    .unwrap();
    let memory = machine.memory();
    // MOV byte [1200],1; JMP 0100. The write aliases data on the RET's page.
    memory
        .view()
        .write(0x100, &[0xc6, 0x06, 0x00, 0x12, 1, 0xeb, 0xf9])
        .unwrap();
    memory.view().write(0x300, &[0xc3]).unwrap();
    let writer = machine.create_cpu(config, None).unwrap();
    let requester = machine.create_cpu(config, Some(compiler)).unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let writing = writer.run_until(stopped, async |mut runner| runner.run_cpu().await);
    let register = async {
        let result = requester
            .run_until(
                futures_timer::Delay::new(Duration::from_secs(20)),
                async move |mut runner| {
                    let memory = runner.cpu().memory();
                    let clock = runner.clock().clone();
                    while memory.view().read_u8(0x200).unwrap() == 0 {
                        clock.sleep(Duration::from_millis(1)).await;
                    }
                    let mut client = runner.take_compiler().unwrap();
                    client
                        .request(
                            runner.cpu().cpu_state().execution_target(),
                            vec![BlockRequest {
                                address: 0x300,
                                start_eip: 0x300,
                                byte_len: 1,
                                instruction_limit: 1,
                            }],
                        )
                        .unwrap();
                    let code = client.recv().await.unwrap().unwrap();
                    while !code.source().has_recorded_writes() {
                        clock.sleep(Duration::from_millis(1)).await;
                    }
                    code.install(runner.cpu()).unwrap(); // Only neighboring data changed.
                },
            )
            .await;
        let _ = stop.send(());
        result
    };
    let (writer, requester) = futures_util::future::join(writing, register).await;
    assert_eq!(writer.unwrap(), None);
    assert_eq!(
        requester.unwrap(),
        Some(()),
        "registration must finish and the writer must resume"
    );
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn registration_interrupts_a_busy_cpu_and_tracks_its_alias_writes() {
    pollster::block_on(compilation::run(register_while_writing));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn registration_interrupts_a_busy_cpu_and_tracks_its_alias_writes() {
    compilation::run(register_while_writing).await;
}
