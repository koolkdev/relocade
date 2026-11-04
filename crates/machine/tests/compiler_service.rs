use futures_channel::oneshot;
use futures_lite::future::{or, zip};
use relocade_machine::{
    yield_now, BlockRequest, Clock, CompilerService, CompilerWorker, CpuConfig, Device,
    DispatchEntry, ExecutionWorker, MachineConfig, Memory,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{
    cell::{Cell, RefCell},
    future::pending,
    rc::Rc,
    time::Duration,
};

struct BusyDevice(Rc<Cell<u64>>);

impl Device for BusyDevice {
    async fn run(device: Rc<RefCell<Self>>) {
        let polls = device.borrow().0.clone();
        loop {
            polls.set(polls.get() + 1);
            yield_now().await;
        }
    }
}

fn block(address: u32) -> Vec<BlockRequest> {
    vec![BlockRequest {
        address,
        start_eip: address,
        byte_len: 3,
        instruction_limit: 1,
    }]
}

async fn check_service() {
    let map =
        PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let config = MachineConfig::new(0x1000, map).unwrap();
    let memory = Memory::new(config.clone()).unwrap();
    memory.view().write(0x100, &[0x40, 0xeb, 0xfd]).unwrap(); // INC AX; JMP 100
    memory.view().write(0x200, &[0x43, 0xeb, 0xfd]).unwrap(); // INC BX; JMP 200
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let mut service = CompilerService::new(CompilerWorker::start(&memory, target).await.unwrap());
    drop(service.connect().unwrap().bind());
    let clients = [service.connect().unwrap(), service.connect().unwrap()];
    let execution = async {
        // Two successive CPU owners share the compiler lifetime and guest memory.
        // The second connection keeps it alive after the first is released.
        for connection in clients {
            let result = ExecutionWorker::run_until(
                pending::<()>(),
                (memory.clone(), connection),
                async move |runner, (memory, connection)| {
                    let mut compiler = connection.bind();
                    let polls = Rc::new(Cell::new(0));
                    let config = CpuConfig::new(
                        target,
                        CpuState {
                            eip: 0x100,
                            segments: Segments::real_mode(),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let mut cpu = runner
                        .create_cpu(memory, config, Default::default())
                        .await
                        .unwrap();
                    let _device = runner
                        .connect(Rc::new(RefCell::new(BusyDevice(polls.clone()))), [], [])
                        .await;
                    let state = cpu.cpu_state();
                    let receive = async {
                        // Abandon only after readiness proves the first resource
                        // has been posted. The next receive must drain it by ID.
                        drop(compiler.request(block(0x100)).await.unwrap());
                        let posted = compiler.request(block(0x200)).await.unwrap();
                        // Hold delivery over several host turns. The guest must
                        // keep advancing before this receive can complete.
                        let (allow, allowed) = oneshot::channel();
                        let progress = async {
                            let mut previous = state.instruction_count();
                            for _ in 0..3 {
                                runner.clock().sleep(Duration::from_millis(3)).await;
                                yield_now().await;
                                assert!(
                                    state.instruction_count() > previous,
                                    "pending delivery must not prevent guest entry"
                                );
                                previous = state.instruction_count();
                            }
                            allow.send(()).unwrap();
                        };
                        zip(
                            runner.wait_for_host(async {
                                allowed.await.unwrap();
                                posted.receive().await.unwrap()
                            }),
                            progress,
                        )
                        .await
                        .0
                    };
                    let compiled = or(receive, async {
                        runner.run_cpu(&mut cpu).await;
                        panic!("guest loop must run until module delivery");
                    })
                    .await;
                    assert!(
                        polls.get() > 0,
                        "device activity runs alongside CPU and delivery"
                    );
                    assert_eq!(compiled.execution_target, target);
                    cpu.install_compiled_module(
                        &compiled.module,
                        &[DispatchEntry {
                            address: 0x200,
                            export: &compiled.entries[0].export,
                        }],
                    )
                    .await
                    .unwrap();
                    state.set_eip(0x200);
                    cpu.interrupt_handle().request();
                    cpu.run().await;
                    assert_eq!(state.bx(), 1);
                    assert_eq!(
                        state.eip(),
                        0x201,
                        "installed prefix stops before the interpreter's branch boundary"
                    );
                },
            )
            .await
            .unwrap();
            assert_eq!(result, Some(()));
        }
    };
    let (shutdown, ()) = zip(service.run(), execution).await;
    shutdown.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn session_compiler_delivers_during_busy_execution_and_outlives_clients() {
    pollster::block_on(check_service());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn session_compiler_delivers_during_busy_execution_and_outlives_clients() {
    check_service().await;
}
