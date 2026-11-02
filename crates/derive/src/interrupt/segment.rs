//! Visible real-mode selectors and their cached segment state.

use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::Ident;

pub(super) struct Segment(Ident);

impl Segment {
    pub(super) fn from_name(name: &Ident) -> Option<Self> {
        matches!(
            name.to_string().as_str(),
            "es" | "cs" | "ss" | "ds" | "fs" | "gs"
        )
        .then(|| Self(name.clone()))
    }

    fn variant(&self) -> Tokens {
        let mut name = self.0.to_string();
        name[..1].make_ascii_uppercase();
        let variant = format_ident!("{name}");
        quote!(::relocade_machine::real_mode_interrupt::Segment::#variant)
    }

    pub(super) fn range(&self) -> Tokens {
        let segment = self.variant();
        quote!(::relocade_machine::real_mode_interrupt::registers::segment(#segment))
    }

    pub(super) fn read(&self, cpu: &Ident) -> Tokens {
        let segment = self.variant();
        quote!(#cpu.segment(#segment).selector)
    }

    pub(super) fn write(&self, call: &Ident, value: Tokens) -> syn::Result<Tokens> {
        if self.0 == "cs" || self.0 == "ss" {
            return Err(syn::Error::new_spanned(
                &self.0,
                "interrupt bindings must preserve code and stack segments",
            ));
        }
        let segment = self.variant();
        Ok(quote! {
            #call.cpu.set_segment(
                #segment,
                ::relocade_machine::real_mode_interrupt::SegmentRegister::real_mode(#segment, #value),
            );
        })
    }
}
