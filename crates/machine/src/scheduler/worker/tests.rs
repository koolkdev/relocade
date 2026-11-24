use super::*;
use crate::{ClockSource, CpuConfig, Machine, MachineConfig, SessionConfig, Sleep, Timestamp};
use futures_channel::oneshot;
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMemoryMap};

#[path = "../../../../workers/tests/support/gate.rs"]
mod gate;
use gate::blocking_gate;

fn cpu() -> (Machine, CpuHandle) {
    let mut machine =
        Machine::new(MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap()).unwrap();
    let cpu = machine
        .create_cpu(
            CpuConfig::new(
                Compiler::new(ExecutionProfile::Real16).config(),
                CpuState::real_mode(),
            )
            .unwrap(),
            None,
        )
        .unwrap();
    (machine, cpu)
}

struct Dropped(Option<oneshot::Sender<()>>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.take().unwrap().send(()).unwrap();
    }
}

// The execution worker reads its supplied clock's tick rate during initialization.
struct StartupClock<F> {
    on_first_read: std::cell::Cell<Option<F>>,
    _cleanup: Dropped,
}

impl<F: FnOnce()> ClockSource for StartupClock<F> {
    fn ticks_per_second(&self) -> std::num::NonZeroU64 {
        if let Some(start) = self.on_first_read.take() {
            start();
        }
        crate::TickRate::NANOSECONDS.ticks_per_second()
    }

    fn now(&self) -> Timestamp {
        Timestamp::from_ticks(0)
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        Sleep::new(async move {
            if deadline > Timestamp::from_ticks(0) {
                std::future::pending::<()>().await;
            }
        })
    }
}

async fn stopped_startup() {
    let (machine, cpu) = cpu();
    let (entered, started) = oneshot::channel();
    let (cleanup, mut cleaned) = oneshot::channel();
    let (release, wait) = blocking_gate();
    let clock = StartupClock {
        on_first_read: std::cell::Cell::new(Some(move || {
            entered.send(()).unwrap();
            wait();
        })),
        _cleanup: Dropped(Some(cleanup)),
    };
    let worker = ExecutionWorker::<()>::spawn(
        cpu,
        SessionConfig::host().with_host_clock(clock),
        async |_| panic!("cancelled construction must not start the session"),
    );

    started.await.unwrap();
    let mut stop = std::pin::pin!(worker.stop());
    assert!(futures_lite::future::poll_once(stop.as_mut())
        .await
        .is_none());
    assert_eq!(cleaned.try_recv().unwrap(), None);
    drop(release);
    assert_eq!(stop.await.unwrap(), None);
    assert_eq!(cleaned.try_recv().unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

async fn cancelled_job_thread_startup() {
    let (machine, cpu) = cpu();
    ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |runner| {
        // End the session even if the browser's startup handshake is pending.
        let _ = futures_lite::future::poll_once(runner.spawn_job_thread()).await;
    })
    .join()
    .await
    .unwrap();
    machine.shutdown().await.unwrap();
}

async fn owned_job_cleanup(cancel: bool) {
    let (machine, cpu) = cpu();
    let (export, exported) = oneshot::channel();
    let (started, ready) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let (cleanup, mut cleaned) = oneshot::channel();
    let (release, wait) = blocking_gate();
    let worker = ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |runner| {
        // A busy activity keeps the execution worker runnable while the
        // browser delivers the calculation worker's startup handshake.
        runner
            .start_task(async {
                loop {
                    crate::yield_now().await;
                }
            })
            .await;
        let jobs = runner.spawn_job_thread().await;
        let mut work = std::pin::pin!(jobs.execute(move || {
            let _cleanup = Dropped(Some(cleanup));
            started.send(()).unwrap();
            wait();
        }));
        assert!(futures_lite::future::poll_once(work.as_mut())
            .await
            .is_none());
        export
            .send(jobs.clone())
            .unwrap_or_else(|_| panic!("job owner"));
        let _ = finished.await;
    });
    let jobs = exported.await.unwrap();
    ready.await.unwrap();
    if !cancel {
        finish.send(()).unwrap();
    }
    let mut shutdown = std::pin::pin!(async move {
        if cancel {
            worker.stop().await
        } else {
            worker.join().await
        }
    });
    assert!(futures_lite::future::poll_once(shutdown.as_mut())
        .await
        .is_none());
    assert_eq!(cleaned.try_recv().unwrap(), None);
    drop(release);
    assert_eq!(shutdown.await.unwrap(), (!cancel).then_some(()));
    assert_eq!(cleaned.try_recv().unwrap(), Some(()));
    assert!(matches!(
        jobs.execute(|| ()).await,
        Err(crate::worker::PoolError::Closed)
    ));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopping_during_construction_drops_startup_before_joining() {
    pollster::block_on(stopped_startup());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stopping_during_construction_drops_startup_before_joining() {
    stopped_startup().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn session_completion_and_cancellation_drain_calculation_jobs() {
    pollster::block_on(async {
        cancelled_job_thread_startup().await;
        owned_job_cleanup(false).await;
        owned_job_cleanup(true).await;
    });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn session_completion_and_cancellation_drain_calculation_jobs() {
    cancelled_job_thread_startup().await;
    owned_job_cleanup(false).await;
    owned_job_cleanup(true).await;
}
