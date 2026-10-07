//! Successful access proofs retained along one execution path.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32};

use super::page_table::{physical_address, FRAME_MASK, PAGE_BYTES};
use super::{Access, Intent, Memory};
use crate::exception::Exception;

/// Virtual mappings must stay fixed until this execution path leaves generated code.
/// Clones inherit dominating proofs; proofs from a child must not escape its region.
/// Guest bytes are always transferred anew, including through physical aliases.
/// Physical accesses retain no routing proofs because MMIO callbacks may remap them.
#[derive(Clone)]
pub(crate) struct Accesses<'memory> {
    memory: &'memory Memory,
    read_anchor: Option<Access>,
    write_anchor: Option<Access>,
}

impl Memory {
    pub(crate) fn accesses(&self) -> Accesses<'_> {
        Accesses {
            memory: self,
            read_anchor: None,
            write_anchor: None,
        }
    }
}

impl<'memory> Accesses<'memory> {
    pub(crate) fn memory(&self) -> &'memory Memory {
        self.memory
    }

    /// Segment checks still belong to each access. A miss performs paging checks
    /// here, so its fault observes exactly the preceding architectural progress.
    pub(crate) fn resolve(
        &mut self,
        body: &mut BlockBuilder<'_>,
        start: &Val<I32>,
        bytes: u32,
        intent: Intent,
        mut on_fault: impl FnMut(BlockBuilder<'_>, Exception<Val<I32>>) -> Result<(), BuildError>,
    ) -> Result<Access, BuildError> {
        assert!((1..=PAGE_BYTES).contains(&bytes));
        if !self.memory.has_stable_mappings() {
            return self.memory.resolve_access(
                body,
                start,
                bytes,
                intent,
                None,
                Some(&mut on_fault),
            );
        }
        let required = intent.required_permissions();
        // An identical checked span also covers narrower accesses, even when
        // the original span crosses a page or has scattered physical backing.
        if let Some(access) = self
            .write_anchor
            .iter()
            .chain(self.read_anchor.iter())
            .find(|access| {
                access.intent.required_permissions() & required == required
                    && access.linear.same_expression(start)
                    && access
                        .constant_bytes
                        .is_some_and(|checked| checked >= bytes)
            })
        {
            return Ok(Access {
                constant_bytes: Some(bytes),
                intent,
                ..access.clone()
            });
        }
        // A fixed anchor avoids chaining page-entry selections through every
        // access. Separate read and write anchors serve both sides of a copy.
        // A write proof also permits reads.
        let anchor = match intent {
            Intent::Read | Intent::Fetch => {
                self.read_anchor.as_ref().or(self.write_anchor.as_ref())
            }
            Intent::Write => self.write_anchor.as_ref(),
        };
        let access = if let Some(anchor) = anchor {
            let page = anchor.linear.and(FRAME_MASK);
            // One unsigned test covers the entire span, including linear wrap.
            let fits = start.sub(&page).unsigned().lt(PAGE_BYTES - bytes + 1);
            let (scattered, physical) = body.if_value::<(I1, I32)>(
                fits,
                |hit| hit.yield_((false, physical_address(&anchor.physical, start))),
                |mut miss| {
                    let access = self.memory.resolve_access(
                        &mut miss,
                        start,
                        bytes,
                        intent,
                        None,
                        Some(&mut on_fault),
                    )?;
                    miss.yield_((access.scattered(), access.physical))
                },
            )?;
            Access {
                linear: start.clone(),
                physical,
                denied: body.value(false)?,
                unavailable: scattered,
                intent,
                constant_bytes: Some(bytes),
            }
        } else {
            self.memory
                .resolve_access(body, start, bytes, intent, None, Some(&mut on_fault))?
        };
        let retained = match intent {
            Intent::Read | Intent::Fetch => &mut self.read_anchor,
            Intent::Write => &mut self.write_anchor,
        };
        if retained.is_none() {
            *retained = Some(access.clone());
        }
        Ok(access)
    }
}
