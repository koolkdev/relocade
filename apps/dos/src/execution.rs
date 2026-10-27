use crate::DosApplication;
use futures_util::future::poll_fn;
use relocade_app::{Context, Exit};
use relocade_dos::{Dos, ProcessStatus};
use relocade_vga::Display;
use relocade_x86::ExecutionExit;
use std::{future::Future, io, task::Poll, time::Duration};
use web_time::Instant;

const RUN_BUDGET: u64 = 10_000;
const DISPLAY_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

impl DosApplication {
    pub(crate) async fn execute(self, context: Context<Display>) -> Result<Exit<u8>, String> {
        let mut dos = Dos::new().await.map_err(|error| error.to_string())?;
        dos.launch_com(&self.program, Default::default())
            .map_err(|error| error.to_string())?;
        let process = dos.process().expect("launched DOS process");
        let mut stdout = dos.stdout();
        let mut stderr = dos.stderr();
        let vga = dos.vga();
        context.send(vga.borrow().display());
        let mut published = Instant::now();

        loop {
            // An always-ready program can finish many slices in one future poll.
            if context.is_stopping() {
                return Ok(Exit::Cancelled);
            }
            let exit = {
                let run = dos.cpu().run(RUN_BUDGET);
                futures_util::pin_mut!(run);
                poll_fn(|task_context| {
                    let poll = run.as_mut().poll(task_context);
                    // VGA already received these writes. Discard DOS's captured
                    // copies so they cannot accumulate during execution.
                    io::copy(&mut stdout, &mut io::sink()).expect("DOS stdout buffer");
                    io::copy(&mut stderr, &mut io::sink()).expect("DOS stderr buffer");
                    // Publish before waits and completion. Ready CPU slices use
                    // this display cadence until VGA owns emulated scanout.
                    if !matches!(&poll, Poll::Ready(ExecutionExit::BudgetExhausted))
                        || published.elapsed() >= DISPLAY_INTERVAL
                    {
                        vga.borrow_mut().publish();
                        published = Instant::now();
                    }
                    poll
                })
                .await
            };
            if let ProcessStatus::Exited(result) = process.status() {
                return result
                    .map(Exit::Completed)
                    .map_err(|error| error.to_string());
            }
            if exit != ExecutionExit::BudgetExhausted {
                return Err(format!("Guest execution stopped: {exit:?}"));
            }
        }
    }
}
