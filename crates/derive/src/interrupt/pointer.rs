//! Segment:offset mappings capture and return real-mode far pointers.

use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{parenthesized, parse::ParseStream, Ident, Token};

pub(super) struct Pointer {
    segment: Ident,
    offset: Ident,
}

impl Pointer {
    pub(super) fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let content;
        parenthesized!(content in input);
        let segment: Ident = content.parse()?;
        if !matches!(
            segment.to_string().as_str(),
            "es" | "cs" | "ss" | "ds" | "fs" | "gs"
        ) {
            return Err(syn::Error::new_spanned(
                segment,
                "expected a segment register",
            ));
        }
        content.parse::<Token![,]>()?;
        let offset = super::register::general_register(&content)?;
        if !content.is_empty() {
            return Err(content.error("expected ptr(segment, offset)"));
        }
        Ok(Self { segment, offset })
    }

    fn segment(&self) -> Tokens {
        let mut name = self.segment.to_string();
        name[..1].make_ascii_uppercase();
        let variant = format_ident!("{name}");
        quote!(::relocade_machine::real_mode_interrupt::Segment::#variant)
    }

    pub(super) fn ranges(&self) -> Vec<Tokens> {
        let segment = self.segment();
        let offset = &self.offset;
        vec![
            quote!(::relocade_machine::real_mode_interrupt::registers::segment(#segment)),
            quote!(::relocade_machine::real_mode_interrupt::registers::#offset),
        ]
    }

    pub(super) fn read(&self, cpu: &Ident) -> Tokens {
        let segment = self.segment();
        let offset = &self.offset;
        quote!(::relocade_machine::real_mode_interrupt::FarPointer {
            segment: #cpu.segment(#segment).selector,
            offset: #cpu.#offset(),
        })
    }

    pub(super) fn write(&self, call: &Ident, value: Tokens) -> syn::Result<Tokens> {
        if self.segment == "cs" || self.segment == "ss" {
            return Err(syn::Error::new_spanned(
                &self.segment,
                "interrupt bindings must preserve code and stack segments",
            ));
        }
        let segment = self.segment();
        let setter = format_ident!("set_{}", self.offset);
        Ok(quote!({
            let pointer: ::relocade_machine::real_mode_interrupt::FarPointer = #value;
            #call.cpu.set_segment(
                #segment,
                ::relocade_machine::real_mode_interrupt::SegmentRegister::real_mode(#segment, pointer.segment),
            );
            #call.cpu.#setter(pointer.offset);
        }))
    }
}
