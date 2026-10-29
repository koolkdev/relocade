use crate::WindowState;
use futures_channel::mpsc;
use winit::dpi::PhysicalSize;

/// Main-thread redraw demand: one draw in flight and one pending request.
/// Latest output state stays here while graphics is busy; no resize backlog is sent.
pub struct Redraw {
    requests: mpsc::Sender<PhysicalSize<u32>>,
    output: WindowState,
    in_flight: bool,
    pending: bool,
}

/// The application owns the sender; the graphics worker awaits the ordinary Rust
/// receiver. Report completion to main after every consumed request, including
/// skipped frames, then call `Redraw::completed` there.
pub fn redraw_channel(output: WindowState) -> (Redraw, mpsc::Receiver<PhysicalSize<u32>>) {
    // A futures mpsc sender has one reserved slot, so capacity zero holds one request.
    let (requests, incoming) = mpsc::channel(0);
    (
        Redraw {
            requests,
            output,
            in_flight: false,
            pending: false,
        },
        incoming,
    )
}

impl Redraw {
    /// Updates layout/visibility without generating redraw demand.
    pub fn output(&mut self, output: WindowState) {
        self.output = output;
    }

    /// Call for an application redraw opportunity, normally winit's RedrawRequested.
    pub fn request(&mut self) -> Result<(), mpsc::TrySendError<PhysicalSize<u32>>> {
        self.pending = true;
        self.dispatch()
    }

    /// Acknowledges one consumed request and dispatches any coalesced demand with
    /// the latest output state. This does not request another frame by itself.
    pub fn completed(&mut self) -> Result<(), mpsc::TrySendError<PhysicalSize<u32>>> {
        self.in_flight = false;
        self.dispatch()
    }

    fn dispatch(&mut self) -> Result<(), mpsc::TrySendError<PhysicalSize<u32>>> {
        if self.pending && !self.in_flight && self.output.drawable() {
            self.requests.try_send(self.output.size)?;
            self.pending = false;
            self.in_flight = true;
        }
        Ok(())
    }
}
