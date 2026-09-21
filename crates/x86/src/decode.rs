mod address16;
mod runtime;
mod snapshot;

pub(super) use runtime::{InstructionFetch, RuntimeDecoder};
pub(super) use snapshot::snapshot;

use crate::instruction::{
    opcode_forms, Form, OpcodeMap, Prefix, PrefixState, EXTENDED_OPCODE_ESCAPE,
};

/// Form selection and opcode-map routing, independent of byte-fetch state.
#[derive(Clone, Default)]
struct DecodeState {
    prefixes: PrefixState,
    map: OpcodeMap,
}

impl DecodeState {
    fn with_prefix(&self, prefix: Prefix) -> Self {
        Self {
            prefixes: self.prefixes.clone().with_prefix(prefix),
            map: self.map,
        }
    }

    fn forms(&self) -> impl Iterator<Item = &'static Form> + Clone {
        let prefixes = self.prefixes.clone();
        opcode_forms(self.map).filter(move |form| form.resolve(&prefixes).is_some())
    }

    /// Reject an unsupported map before requesting its selector byte.
    fn extended(&self) -> Option<Self> {
        let extended = Self {
            map: OpcodeMap::Extended,
            ..self.clone()
        };
        extended.forms().next().map(|_| extended)
    }

    fn unsupported_opcode_override(&self) -> Option<u8> {
        if let Some(prefix) = self.prefixes.group1() {
            Some(prefix.byte())
        } else if self.map == OpcodeMap::Extended {
            Some(EXTENDED_OPCODE_ESCAPE)
        } else {
            None
        }
    }
}
