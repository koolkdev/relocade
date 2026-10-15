//! Shared host replies for guest-visible system-register queries.

use crate::{DescriptorTable, DescriptorTableRegister};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub(crate) struct DescriptorTableQuery {
    pub(crate) table: u32,
    pub(crate) base: u32,
    pub(crate) limit: u16,
}

impl DescriptorTableQuery {
    pub(crate) fn new(table: DescriptorTable, register: DescriptorTableRegister) -> Self {
        Self {
            table: table as u32,
            base: register.base,
            limit: register.limit,
        }
    }
}
