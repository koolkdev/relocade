#![cfg(not(target_arch = "wasm32"))]

use futures_util::StreamExt;
use relocade_app::{spawn, Application, Context};
use relocade_machine::timer_service;
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

struct Fixture {
    started: mpsc::Sender<()>,
    dropped: Arc<AtomicBool>,
}
struct LocalState(Arc<AtomicBool>);
impl Drop for LocalState {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl Application for Fixture {
    type Input = u8;
    type Source = &'static str;
    type Output = u8;
    async fn run(self, mut context: Context<u8, &'static str>) -> Result<u8, String> {
        let state = Rc::new(LocalState(self.dropped));
        context.publish("ready");
        self.started.send(()).unwrap();
        let input = context.input.next().await.unwrap();
        context.publish("finished");
        drop(state);
        Ok(input)
    }
}
#[test]
fn input_and_retained_sources_are_independent_of_a_window() {
    let (clock, _timers) = timer_service();
    let (started, ready) = mpsc::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let mut task = spawn(
        Fixture {
            started,
            dropped: dropped.clone(),
        },
        clock,
    );
    ready
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    assert_eq!(task.current_source().as_deref(), Some(&"ready"));
    assert!(task.send_input(7));
    assert_eq!(pollster::block_on(&mut task).unwrap(), Some(7));
    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(task.current_source().as_deref(), Some(&"finished"));
    assert!(!task.send_input(8));
}
#[test]
fn cancellation_drops_worker_local_state_before_completion() {
    let (clock, _timers) = timer_service();
    let (started, ready) = mpsc::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let mut task = spawn(
        Fixture {
            started,
            dropped: dropped.clone(),
        },
        clock,
    );
    ready
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    task.stop();
    assert_eq!(pollster::block_on(&mut task).unwrap(), None);
    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(task.current_source().as_deref(), Some(&"ready"));
}
