use futures_channel::oneshot;
use futures_lite::future::or;
use relocade_machine::{
    timer_service, worker, yield_now, BlockRequest, CompilerClient, CompilerConnection,
    CompilerWorker, CpuConfig, Device, DispatchEntry, ExecutionWorker, GuestTimeConfig,
    MachineConfig, Memory, Runner,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{
    cell::{Cell, RefCell},
    num::NonZeroU64,
    rc::Rc,
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

async fn check_connections() {
    let memory = Memory::new(
        MachineConfig::new(
            0x1000,
            PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    memory.view().write(0x100, &[0x40, 0xeb, 0xfd]).unwrap();
    memory.view().write(0x200, &[0x43, 0xeb, 0xfd]).unwrap();
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let mut compiler = CompilerWorker::start(&memory, target).unwrap();
    let (client, connection) = CompilerClient::channel().unwrap();
    compiler.connect(connection);
    drop(client);

    // The same compiler thread outlives successive execution owners. Neither
    // connect nor compilation relies on polling the compiler's shutdown future.
    for guest_time in [false, true] {
        let (connections, incoming) = worker::channel::<CompilerConnection>().unwrap();
        let mut incoming = incoming.bind();
        let (send_setup, receive_setup) =
            worker::oneshot::channel::<(Memory, worker::Sender<CompilerConnection>)>();
        let (started, ready) = oneshot::channel();
        let (timers, driver) = timer_service();
        let session = async move |runner: &Runner| {
            started.send(()).unwrap();
            let (memory, mut connections) = runner.receive_resource(receive_setup).await.unwrap();
            let mut cpu = runner
                .create_cpu(
                    memory,
                    CpuConfig::new(
                        target,
                        CpuState {
                            eip: 0x100,
                            segments: Segments::real_mode(),
                            ..Default::default()
                        },
                    )
                    .unwrap(),
                    Default::default(),
                )
                .await
                .unwrap();
            let polls = Rc::new(Cell::new(0));
            let _device = runner
                .connect(Rc::new(RefCell::new(BusyDevice(polls.clone()))), [], [])
                .await;
            let state = cpu.cpu_state();
            let (mut client, connection) = runner.compiler_connection().unwrap();
            let attach = async {
                while state.instruction_count() == 0 {
                    yield_now().await;
                }
                connections.send(connection).unwrap();
                client.compile(block(0x100)).await.unwrap()
            };
            let first = or(attach, async {
                runner.run_cpu(&mut cpu).await;
                panic!("guest loop must run until compilation completes");
            })
            .await;
            assert!(state.instruction_count() > 0);
            assert_eq!(first.execution_target, target);
            if guest_time {
                // Resource delivery progresses even with guest time paused and
                // a continuously runnable device. It must not advance that clock.
                let before = runner.clock().now();
                let count = state.instruction_count();
                assert_eq!(
                    client.compile(block(0x100)).await.unwrap().entries[0].start_eip,
                    0x100
                );
                assert_eq!(state.instruction_count(), count);
                assert_eq!(runner.clock().now(), before);
            }
            let receive = async {
                let mut invalid = block(0x100);
                invalid[0].instruction_limit = 0;
                assert!(client.compile(invalid).await.is_err());
                client.compile(block(0x200)).await.unwrap()
            };
            let compiled = or(receive, async {
                runner.run_cpu(&mut cpu).await;
                panic!("guest loop must run until module delivery");
            })
            .await;
            assert!(polls.get() > 0);
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
            assert_eq!((state.bx(), state.eip()), (1, 0x201));
        };
        let execution = if guest_time {
            ExecutionWorker::spawn_with_guest_time(
                || GuestTimeConfig::new(NonZeroU64::new(1_000_000).unwrap()),
                session,
            )
        } else {
            ExecutionWorker::spawn(timers, session)
        };
        let driver = driver.spawn_with_peer(execution.wake_peer());
        ready.await.unwrap();
        execution
            .send_resource(send_setup, (memory.clone(), connections))
            .await
            .unwrap();
        compiler.connect(incoming.recv().await.unwrap().unwrap());
        assert_eq!(execution.join().await.unwrap(), Some(()));
        driver.stop().await.unwrap();
    }
    compiler.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compiler_connections_deliver_during_execution_and_outlive_clients() {
    pollster::block_on(check_connections());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compiler_connections_deliver_during_execution_and_outlive_clients() {
    check_connections().await;
}
