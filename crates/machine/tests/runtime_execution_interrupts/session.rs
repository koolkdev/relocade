use super::*;
use futures_channel::{mpsc, oneshot};
use futures_util::{
    future::{join, LocalBoxFuture},
    StreamExt,
};
use relocade_machine::{timer_service, Device, ExecutionInterrupt, ExitHandle};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

struct InputDevice {
    entered: Option<oneshot::Sender<()>>,
    sent: Arc<AtomicBool>,
    input: Option<LocalBoxFuture<'static, Vec<u8>>>,
    values: Vec<u8>,
    exit: Option<ExitHandle>,
    interrupt: Option<ExecutionInterrupt>,
}

impl Device for InputDevice {
    fn write_port(&mut self, _port: u16, _bytes: u8, _value: u32) {
        if let Some(entered) = self.entered.take() {
            // Discard startup scheduling wakes before the measured interval.
            // Only the following host notification may interrupt this dispatch.
            self.interrupt.as_ref().unwrap().take();
            entered.send(()).unwrap();
            // Keep this first guest block in progress until the host publishes
            // its ordinary Rust notification. No test calls interrupt.request().
            // Timer-worker startup is asynchronous. Bound this fixture by elapsed
            // time rather than a spin count whose speed differs across engines.
            let deadline = web_time::Instant::now() + Duration::from_secs(5);
            while web_time::Instant::now() < deadline {
                if self.sent.load(Ordering::SeqCst) {
                    return;
                }
                std::hint::spin_loop();
            }
            panic!("host did not publish its input");
        }
    }

    async fn run(device: Rc<RefCell<Self>>) {
        let input = device.borrow_mut().input.take().unwrap();
        let values = input.await;
        let mut device = device.borrow_mut();
        device.values = values;
        if let Some(exit) = &device.exit {
            exit.request();
        }
    }
}

#[derive(Clone, Copy)]
enum InputSource {
    Host,
    Timer,
    Peer,
}

async fn connected_input() {
    for compiled in [false, true] {
        for source in [InputSource::Host, InputSource::Timer, InputSource::Peer] {
            let timed = matches!(source, InputSource::Timer);
            let (clock, timers) = timer_service();
            let delivery = clock.clock();
            let (entered, running) = oneshot::channel();
            let sent = Arc::new(AtomicBool::new(false));
            let worker_sent = sent.clone();
            let (send, mut input) = mpsc::unbounded();
            let (start, started) = oneshot::channel();
            let setup = configuration();
            let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
            let prepared = machine.create_cpu(setup.cpu, None).unwrap();
            let worker = prepared.spawn(clock, async move |mut runner| {
                let input: LocalBoxFuture<'static, Vec<u8>> = if timed {
                    let clock = runner.clock().clone();
                    Box::pin(async move {
                        clock.sleep(Duration::ZERO).await;
                        vec![7]
                    })
                } else {
                    Box::pin(async move {
                        vec![input.next().await.unwrap(), input.next().await.unwrap()]
                    })
                };
                let device = Rc::new(RefCell::new(InputDevice {
                    entered: Some(entered),
                    sent: worker_sent,
                    input: Some(input),
                    values: Vec::new(),
                    exit: None,
                    interrupt: None,
                }));
                let io = runner.cpu().io();
                io.connect([runner
                    .connect(device.clone(), [], std::iter::once(0x80..=0x80))
                    .await]);
                // OUT; JMP 0100. I/O returns through interpreter STEP after OUT.
                prepare_cpu(&mut runner, &[0xe6, 0x80, 0xeb, 0xfc], compiled).await;
                device.borrow_mut().exit = Some(runner.cpu().exit_handle());
                device.borrow_mut().interrupt = Some(runner.cpu().interrupt_handle());
                started.await.unwrap();
                assert_eq!(
                    runner.run_cpu().await,
                    ExecutionExit::Host(relocade_x86::HostExit::Return)
                );
                let values = device.borrow().values.clone();
                (runner.cpu().cpu_state().instruction_count(), values)
            });
            use relocade_machine::worker::{Receiver, Thread, WakePeer};
            let peer_thread = matches!(source, InputSource::Peer).then(Thread::new);
            let peer_task = if let Some(thread) = &peer_thread {
                let peer = worker.wake_peer();
                let send = send.clone();
                let sent = sent.clone();
                let (ready, bound) = oneshot::channel();
                let (publish, published) = oneshot::channel();
                let (mut input, task) = thread
                    .listen(
                        move |mut input: Receiver<
                            WakePeer<relocade_machine::ExecutionInterrupt>,
                        >| async move {
                            let peer = input.recv().await.unwrap().unwrap();
                            let _binding = peer.bind();
                            ready.send(()).unwrap();
                            published.await.unwrap();
                            send.unbounded_send(4).unwrap();
                            send.unbounded_send(9).unwrap();
                            sent.store(true, Ordering::SeqCst);
                        },
                    )
                    .unwrap();
                input.send(peer).unwrap();
                bound.await.unwrap();
                Some((publish, task))
            } else {
                None
            };
            // Finish peer setup before the measured busy interval. Thread startup
            // latency is independent of notification delivery at a checkpoint.
            start.send(()).unwrap();
            running.await.unwrap();
            let timers = timers.spawn_with_peer(worker.wake_peer());
            if timed {
                // This later request completes after the device's queued timer.
                // Keep guest dispatch held until its notification has arrived.
                delivery.sleep(Duration::ZERO).await;
            } else if let Some((publish, task)) = peer_task {
                publish.send(()).unwrap();
                task.await.unwrap();
            } else {
                // The second send can coalesce with the first task wake. Both
                // values must drain before the guest runs another block.
                send.unbounded_send(4).unwrap();
                send.unbounded_send(9).unwrap();
            }
            sent.store(true, Ordering::SeqCst);
            drop(delivery);
            let (result, timers) = join(worker.join(), timers.join()).await;
            timers.unwrap();
            assert_eq!(
                result.unwrap(),
                Some((1, if timed { vec![7] } else { vec![4, 9] })),
                "input must run at the first notified checkpoint"
            );
            machine.shutdown().await.unwrap();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn session_channels_and_timers_interrupt_guest_execution() {
    pollster::block_on(connected_input());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn session_channels_and_timers_interrupt_guest_execution() {
    connected_input().await;
}

async fn connected_stop() {
    let (clock, _timers) = timer_service();
    let (entered, running) = oneshot::channel();
    let sent = Arc::new(AtomicBool::new(false));
    let worker_sent = sent.clone();
    let (report, stopped) = oneshot::channel();
    let setup = configuration();
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let worker = prepared.spawn::<()>(clock, async move |mut runner| {
        let device = Rc::new(RefCell::new(InputDevice {
            entered: Some(entered),
            sent: worker_sent,
            input: Some(Box::pin(std::future::pending())),
            values: Vec::new(),
            exit: None,
            interrupt: None,
        }));
        let io = runner.cpu().io();
        io.connect([runner
            .connect(device.clone(), [], std::iter::once(0x80..=0x80))
            .await]);
        prepare_cpu(&mut runner, &[0xe6, 0x80, 0xeb, 0xfc], true).await;
        device.borrow_mut().interrupt = Some(runner.cpu().interrupt_handle());
        let _stopped = Stopped {
            state: runner.cpu().cpu_state(),
            report: Some(report),
        };
        runner.run_cpu().await;
        panic!("cancellation must drop the running session");
    });
    running.await.unwrap();
    let mut stop = std::pin::pin!(worker.stop());
    assert!(futures_lite::future::poll_once(stop.as_mut())
        .await
        .is_none());
    sent.store(true, Ordering::SeqCst);
    assert_eq!(stop.await.unwrap(), None);
    assert_eq!(
        stopped.await.unwrap(),
        1,
        "stop must interrupt the first guest dispatch"
    );
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopping_a_busy_session_uses_the_same_execution_interrupt() {
    pollster::block_on(connected_stop());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stopping_a_busy_session_uses_the_same_execution_interrupt() {
    connected_stop().await;
}

async fn saturated_count() {
    let (clock, _timers) = timer_service();
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            instruction_count: u64::MAX,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut machine = relocade_machine::Machine::new(config.machine).unwrap();
    let prepared = machine.create_cpu(config.cpu, None).unwrap();
    let worker = prepared.spawn(clock, async |mut runner| {
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([relocade_machine::Hook::host(0x100, async move || {
                exit.request()
            })])
            .await
            .unwrap();
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(relocade_x86::HostExit::Return),
            "the retirement statistic must not prevent entering a host service"
        );
    });
    assert_eq!(worker.join().await.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retirement_count_does_not_control_execution() {
    pollster::block_on(saturated_count());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retirement_count_does_not_control_execution() {
    saturated_count().await;
}

struct PublishInput(Option<oneshot::Sender<()>>);
impl Device for PublishInput {
    fn write_port(&mut self, _port: u16, _bytes: u8, _value: u32) {
        if let Some(input) = self.0.take() {
            input.send(()).unwrap();
        }
    }
}

async fn notified_reentry() {
    for nested in [false, true] {
        let (clock, _timers) = timer_service();
        let setup = configuration();
        let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
        let prepared = machine.create_cpu(setup.cpu, None).unwrap();
        let worker = prepared.spawn(clock, async move |mut runner| {
            let (send, receive) = oneshot::channel();
            let io = runner.cpu().io();
            io.connect([runner
                .connect(
                    Rc::new(RefCell::new(PublishInput(Some(send)))),
                    [],
                    std::iter::once(0x80..=0x80),
                )
                .await]);
            // OUT publishes input and exits through STEP. The following JMP
            // supplies the next guest checkpoint if input needs another poll.
            prepare_cpu(&mut runner, &[0xe6, 0x80, 0xeb, 0xfc], true).await;
            let state = runner.cpu().cpu_state();
            let exit = runner.cpu().exit_handle();
            let input = async {
                receive.await.unwrap();
                // Ordinary async composition can need another poll after input
                // arrives. Its wake is known before the next guest entry.
                relocade_machine::yield_now().await;
                let count = state.instruction_count();
                exit.request();
                count
            };
            let execution = async {
                if nested {
                    let mut tasks = futures_util::stream::FuturesUnordered::new();
                    tasks.push(runner.run_cpu());
                    tasks.next().await.unwrap()
                } else {
                    runner.run_cpu().await
                }
            };
            let (exit, count) = join(execution, input).await;
            assert_eq!(exit, ExecutionExit::Host(relocade_x86::HostExit::Return));
            assert!(
                (1..=2).contains(&count),
                "ready Rust work must run by the next guest checkpoint; retired {count} instructions"
            );
        });
        assert_eq!(worker.join().await.unwrap(), Some(()));
        machine.shutdown().await.unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn notifications_before_guest_reentry_use_the_next_checkpoint() {
    pollster::block_on(notified_reentry());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn notifications_before_guest_reentry_use_the_next_checkpoint() {
    notified_reentry().await;
}
