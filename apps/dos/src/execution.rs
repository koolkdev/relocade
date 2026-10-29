use super::{application::Event, keyboard::KeyboardInput};
use futures_channel::mpsc;
use relocade_app::winit::{event::WindowEvent, event_loop::EventLoopProxy};
use relocade_dos::Dos;
use relocade_machine::Runner;
use std::{
    future::{pending, poll_fn, Future},
    io,
    pin::pin,
};

pub(super) async fn run(
    runner: &Runner,
    program: Vec<u8>,
    mut input: mpsc::UnboundedReceiver<WindowEvent>,
    events: EventLoopProxy<Event>,
) -> Result<(), String> {
    let mut dos = Dos::new(runner).await.map_err(|e| e.to_string())?;
    if program.starts_with(b"MZ") {
        dos.launch_mz(&program, Default::default())
    } else {
        dos.launch_com(&program, Default::default())
    }
    .map_err(|e| e.to_string())?;
    let _ = events.send_event(Event::Frames(dos.frame_source()));
    let mut keyboard = KeyboardInput::new(dos.keyboard());
    let receive = async {
        while let Ok(event) = input.recv().await {
            keyboard.handle(event);
        }
        pending::<Result<(), String>>().await
    };
    let execute = async {
        let (mut stdout, mut stderr) = (dos.stdout(), dos.stderr());
        let code = {
            let mut program = pin!(dos.run(runner));
            poll_fn(|cx| {
                let result = program.as_mut().poll(cx);
                // VGA already owns console output. Do not retain a second unbounded log.
                io::copy(&mut stdout, &mut io::sink()).expect("DOS console buffer read");
                io::copy(&mut stderr, &mut io::sink()).expect("DOS console buffer read");
                result
            })
            .await
            .map_err(|e| e.to_string())?
        };
        let _ = events.send_event(Event::ProgramExited(code));
        // Keep DOS and its VGA activity alive after program exit. ExecutionWorker::stop
        // cancels this session and drops its devices on the execution owner.
        pending::<()>().await;
        Ok(())
    };
    futures_lite::future::or(receive, execute).await
}
