use relocade_dos::{devices::Printer, Dos};
use relocade_machine::Runner;
use relocade_parallel::Centronics;
use relocade_pc::{Configuration, Pc};
use relocade_printer::Capture;
use std::{error::Error, num::NonZeroUsize};

pub(super) const CAPACITY: NonZeroUsize = NonZeroUsize::new(64 * 1024).unwrap();

/// Starts DOS with a capture printer on its configured parallel port.
/// Keeping the capture handle alive keeps the peripheral connected.
pub(super) async fn start(
    runner: Runner,
    configuration: Configuration,
) -> Result<(Dos, Option<Capture>), Box<dyn Error>> {
    let pc = Pc::with_configuration(runner, configuration).await?;
    let Some(port) = pc.parallel_port() else {
        return Ok((Dos::from_pc(pc).await?, None));
    };

    let (input, output) = Centronics::connect(pc.runner(), &port).await;
    let capture = Capture::attach(pc.runner(), input, CAPACITY).await;
    let dos = Dos::from_pc(pc).await?;
    dos.bind_lpt(1, output.clone())?;
    dos.install_driver(Printer::new(output));
    Ok((dos, Some(capture)))
}

#[cfg(test)]
pub(super) mod tests;
