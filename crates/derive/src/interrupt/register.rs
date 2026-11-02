//! Register syntax. Alias ranges and widths come from the CPU accessors.

use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    token, Ident, Token,
};

pub(super) enum Register {
    Scalar(Ident),
    Pair(Ident, Ident),
    Flag(Ident),
}

impl Parse for Register {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(token::Paren) {
            let content;
            parenthesized!(content in input);
            let high = general_register(&content)?;
            content.parse::<Token![,]>()?;
            let low = general_register(&content)?;
            if !content.is_empty() {
                return Err(content.error("expected a high, low word pair"));
            }
            return Ok(Self::Pair(high, low));
        }
        if input.peek(Ident) {
            let name: Ident = input.fork().parse()?;
            if name == "cf" || name == "zf" {
                return Ok(Self::Flag(input.parse()?));
            }
        }
        Ok(Self::Scalar(general_register(input)?))
    }
}

fn general_register(input: ParseStream<'_>) -> syn::Result<Ident> {
    let name: Ident = input.parse()?;
    if name == "sp" || name == "esp" {
        return Err(syn::Error::new_spanned(
            name,
            "interrupt bindings must preserve the stack pointer",
        ));
    }
    Ok(name)
}

impl Register {
    pub(super) fn ranges(&self) -> Vec<Tokens> {
        let names: Vec<_> = match self {
            Self::Scalar(name) => vec![name],
            Self::Pair(high, low) => vec![high, low],
            Self::Flag(_) => vec![],
        };
        names
            .into_iter()
            .map(|name| quote!(::relocade_machine::real_mode_interrupt::registers::#name))
            .collect()
    }

    pub(super) fn read(&self, cpu: &Ident) -> syn::Result<Tokens> {
        let ranges = self.ranges();
        let read = match self {
            Self::Scalar(name) => quote!(#cpu.#name()),
            Self::Pair(high, low) => quote!({
                let high: u16 = #cpu.#high();
                let low: u16 = #cpu.#low();
                (u32::from(high) << 16) | u32::from(low)
            }),
            Self::Flag(name) => {
                return Err(syn::Error::new_spanned(
                    name,
                    "saved flags are return values",
                ))
            }
        };
        Ok(quote!(({
            const _: () = { ::relocade_machine::real_mode_interrupt::registers::assert_disjoint(&[#(#ranges),*]); };
            #read
        })))
    }

    pub(super) fn write(&self, call: &Ident, value: Tokens) -> Tokens {
        match self {
            Self::Scalar(name) => {
                let setter = format_ident!("set_{name}");
                quote!(#call.cpu.#setter(#value);)
            }
            Self::Pair(high, low) => {
                let high = format_ident!("set_{high}");
                let low = format_ident!("set_{low}");
                quote!({
                    let pair: u32 = #value;
                    #call.cpu.#high((pair >> 16) as u16);
                    #call.cpu.#low(pair as u16);
                })
            }
            Self::Flag(name) => {
                let setter = if name == "cf" {
                    quote!(set_carry)
                } else {
                    quote!(set_zero)
                };
                quote!(::relocade_machine::real_mode_interrupt::#setter(#call.cpu, &mut #call.memory, #value);)
            }
        }
    }
}
