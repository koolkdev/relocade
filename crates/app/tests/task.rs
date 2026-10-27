#![cfg(not(target_arch = "wasm32"))]

use futures_channel::oneshot;
use relocade_app::{spawn, Application, Context, Exit};
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

enum Work {
    Gate(oneshot::Receiver<()>),
    Pending,
    Running,
}

struct Fixture {
    work: Work,
    dropped: Arc<AtomicUsize>,
}

// Neither the output nor the worker-local state needs Default or Clone.
#[derive(Debug, Eq, PartialEq)]
struct Report(&'static str);

struct LocalState(Arc<AtomicUsize>);

impl Drop for LocalState {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

enum Update {
    Started(Arc<AtomicUsize>),
    Finished,
}

impl Application for Fixture {
    type Update = Update;
    type Output = Report;

    async fn run(self, context: Context<Self::Update>) -> Result<Exit<Report>, String> {
        let local = Rc::new(LocalState(self.dropped.clone()));
        context.send(Update::Started(self.dropped));
        match self.work {
            Work::Gate(gate) => gate.await.map_err(|error| error.to_string())?,
            Work::Pending => std::future::pending::<()>().await,
            Work::Running => {
                while !context.is_stopping() {
                    std::hint::spin_loop();
                }
                return Ok(Exit::Cancelled);
            }
        }
        context.send(Update::Finished);
        drop(local);
        Ok(Exit::Completed(Report("worker result")))
    }
}

struct Running {
    task: relocade_app::Task<Report>,
    updates: mpsc::Receiver<Update>,
    dropped: Arc<AtomicUsize>,
}

fn start(work: Work) -> Running {
    let dropped = Arc::new(AtomicUsize::new(0));
    let (sender, updates) = mpsc::channel();
    let task = spawn(
        Fixture {
            work,
            dropped: dropped.clone(),
        },
        move |update| {
            sender.send(update).unwrap();
        },
    );
    match updates.recv_timeout(Duration::from_secs(60)).unwrap() {
        Update::Started(state) => assert!(Arc::ptr_eq(&state, &dropped)),
        Update::Finished => panic!("expected initialized worker state"),
    }
    Running {
        task,
        updates,
        dropped,
    }
}

#[test]
fn local_future_publishes_custom_updates_and_returns_its_own_output_type() {
    let (release, gate) = oneshot::channel();
    let Running {
        task,
        updates,
        dropped,
    } = start(Work::Gate(gate));
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    release.send(()).unwrap();
    assert_eq!(
        pollster::block_on(task).unwrap(),
        Exit::Completed(Report("worker result"))
    );
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(matches!(
        updates.recv_timeout(Duration::from_secs(60)).unwrap(),
        Update::Finished
    ));
}

#[test]
fn cancellation_drops_pending_and_cooperating_ready_futures_before_completion() {
    for work in [Work::Pending, Work::Running] {
        let Running { task, dropped, .. } = start(work);
        task.stop();
        assert_eq!(pollster::block_on(task).unwrap(), Exit::Cancelled);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
}
