//! Resources shared by successive DOS processes.

use crate::{files::FileSystem, process::Process, stdio::Stdio};
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct DosState {
    pub(crate) filesystem: FileSystem,
    pub(crate) stdio: Stdio,
    pub(crate) process: Option<Rc<Process>>,
}
