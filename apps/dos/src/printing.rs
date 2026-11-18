use futures_channel::oneshot;
use relocade_printer::{text, Capture};
use std::io;

const PDF_FILENAME: &str = "relocade-printout.pdf";

pub(super) struct SavePrintout {
    pub pdf: Vec<u8>,
    pub complete: oneshot::Sender<io::Result<Option<()>>>,
}

impl SavePrintout {
    pub async fn save(self) {
        let result = relocade_file_picker::save_file(PDF_FILENAME, self.pdf).await;
        let _ = self.complete.send(result);
    }
}

/// A captured job that the execution worker can export through the UI thread.
pub(super) struct Printout {
    pub capture: Capture,
    pub saves: async_channel::Sender<SavePrintout>,
}

impl Printout {
    pub async fn save_pdf(&self) -> Result<(), String> {
        let bytes = self
            .capture
            .snapshot()
            .await
            .map_err(|error| error.to_string())?;
        let pdf = text::render_pdf(&bytes)?;
        let (complete, completed) = oneshot::channel();
        self.saves
            .send(SavePrintout { pdf, complete })
            .await
            .map_err(|_| "The file picker has closed")?;
        completed
            .await
            .map_err(|_| "The file picker has closed")?
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}
