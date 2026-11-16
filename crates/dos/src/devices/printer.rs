//! Resident DOS printer access over an explicitly supplied byte connection.

use super::{parallel::ParallelOutput, CharacterDriver, DriverFuture};
use relocade_parallel::centronics::Output;

/// A PRN character driver that retains its configured forward connection.
/// Writes preserve binary data and await the same capacity and readiness as
/// port transfers. Reads are unsupported. Constructing the driver does not
/// install it; install before launching a process to bind printer handle 4.
///
/// The connection may also back a numbered LPT driver. Installing either driver
/// leaves the other's destination and existing DOS handles unchanged.
///
/// ```no_run
/// # async fn example(pc: relocade_pc::Pc) -> Result<(relocade_dos::Dos, relocade_parallel::centronics::Input), Box<dyn std::error::Error>> {
/// use relocade_dos::devices::Printer;
///
/// let port = pc.parallel_port().expect("configured parallel port");
/// let (input, output) = relocade_parallel::Centronics::connect(pc.runner(), &port).await;
/// let dos = relocade_dos::Dos::from_pc(pc).await?;
/// dos.bind_lpt(1, output.clone())?;
/// dos.install_driver(Printer::new(output));
/// // Give `input` to the peripheral, which consumes bytes through AsyncRead.
/// # Ok((dos, input))
/// # }
/// ```
pub struct Printer {
    output: ParallelOutput,
}

impl Printer {
    /// Uses this connection for every request, independently of LPT registration.
    pub fn new(output: Output) -> Self {
        Self {
            output: ParallelOutput::new(output),
        }
    }
}

impl CharacterDriver for Printer {
    fn name(&self) -> [u8; 8] {
        *b"PRN     "
    }

    fn output_ready(&self) -> DriverFuture<'_, bool> {
        Box::pin(async { Ok(self.output.ready()) })
    }

    fn write<'a>(&'a self, bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        Box::pin(self.output.write(bytes))
    }
}
