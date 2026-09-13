mod runtime;
mod snapshot;

pub(super) use runtime::RuntimeDecoder;
pub(super) use snapshot::snapshot;

use crate::instruction::{
    opcode_forms, Form, OpcodeMap, Prefix, PrefixState, EXTENDED_OPCODE_ESCAPE,
};

/// Form selection and opcode-map routing, independent of byte-fetch state.
#[derive(Clone, Copy, Default)]
struct DecodeState {
    prefixes: PrefixState,
    map: OpcodeMap,
}

impl DecodeState {
    fn with_prefix(self, prefix: Prefix) -> Self {
        Self {
            prefixes: self.prefixes.with_prefix(prefix),
            ..self
        }
    }

    fn forms(self) -> impl Iterator<Item = &'static Form> + Clone {
        opcode_forms(self.map).filter(move |form| form.resolve(self.prefixes).is_some())
    }

    /// Reject an unsupported map before requesting its selector byte.
    fn extended(self) -> Option<Self> {
        let extended = Self {
            map: OpcodeMap::Extended,
            ..self
        };
        extended.forms().next().map(|_| extended)
    }

    fn unsupported_opcode_override(self) -> Option<u8> {
        if self.prefixes.has_f3() {
            Some(Prefix::F3.byte())
        } else if self.map == OpcodeMap::Extended {
            Some(EXTENDED_OPCODE_ESCAPE)
        } else {
            None
        }
    }
}
