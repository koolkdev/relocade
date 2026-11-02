//! Segment:offset mappings capture and return real-mode far pointers.

use super::segment::Segment;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{parenthesized, parse::ParseStream, Ident, Token};

pub(super) struct Pointer {
    segment: Segment,
    offset: Ident,
}

impl Pointer {
    pub(super) fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let content;
        parenthesized!(content in input);
        let name: Ident = content.parse()?;
        let segment = Segment::from_name(&name)
            .ok_or_else(|| syn::Error::new_spanned(name, "expected a segment register"))?;
        content.parse::<Token![,]>()?;
        let offset = super::register::general_register(&content)?;
        if !content.is_empty() {
            return Err(content.error("expected ptr(segment, offset)"));
        }
        Ok(Self { segment, offset })
    }

    pub(super) fn ranges(&self) -> Vec<Tokens> {
        let offset = &self.offset;
        vec![
            self.segment.range(),
            quote!(::relocade_machine::real_mode_interrupt::registers::#offset),
        ]
    }

    pub(super) fn read(&self, cpu: &Ident) -> Tokens {
        let segment = self.segment.read(cpu);
        let offset = &self.offset;
        quote!(::relocade_machine::real_mode_interrupt::FarPointer {
            segment: #segment,
            offset: #cpu.#offset(),
        })
    }

    pub(super) fn write(&self, call: &Ident, value: Tokens) -> syn::Result<Tokens> {
        let segment = self.segment.write(call, quote!(pointer.segment))?;
        let setter = format_ident!("set_{}", self.offset);
        Ok(quote!({
            let pointer: ::relocade_machine::real_mode_interrupt::FarPointer = #value;
            #segment
            #call.cpu.#setter(pointer.offset);
        }))
    }
}
