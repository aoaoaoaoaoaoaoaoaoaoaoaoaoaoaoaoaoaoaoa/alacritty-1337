#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        unused_crate_dependencies,
        unused_results,
        reason = "tests use deliberate failure shortcuts and discard fixture mutations"
    )
)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use std::mem;
use syn::punctuated::Punctuated;
use syn::{Attribute, Error, GenericParam, LitStr, Token, TypeParam};

mod config_deserialize;
mod serde_replace;

/// Error message when attempting to flatten multiple fields.
pub(crate) const MULTIPLE_FLATTEN_ERROR: &str =
    "At most one instance of #[config(flatten)] is supported";

#[proc_macro_derive(ConfigDeserialize, attributes(config))]
pub fn derive_config_deserialize(input: TokenStream) -> TokenStream {
    config_deserialize::derive(input)
}

#[proc_macro_derive(SerdeReplace)]
pub fn derive_serde_replace(input: TokenStream) -> TokenStream {
    serde_replace::derive(input)
}

/// Storage for all necessary generics information.
#[derive(Default)]
struct GenericsStreams {
    unconstrained: TokenStream2,
    constrained: TokenStream2,
    phantoms: TokenStream2,
}

/// Create the necessary generics annotations.
///
/// This will create three different token streams, which might look like this:
///  - unconstrained: `T`
///  - constrained: `T: Default + Deserialize<'de>`
///  - phantoms: `T: PhantomData<T>,`
pub(crate) fn generics_streams<T>(params: &Punctuated<GenericParam, T>) -> GenericsStreams {
    let mut generics = GenericsStreams::default();

    for generic in params {
        // NOTE: Lifetimes and const params are not supported.
        if let GenericParam::Type(TypeParam { ident, .. }) = generic {
            generics.unconstrained.extend(quote!( #ident , ));
            generics.constrained.extend(quote! {
                #ident : Default + serde::Deserialize<'de> + alacritty_config::SerdeReplace,
            });
            generics.phantoms.extend(quote! {
                #ident : std::marker::PhantomData < #ident >,
            });
        }
    }

    generics
}

#[derive(Default)]
pub(crate) struct ConfigAttrs {
    pub skip: bool,
    pub flatten: bool,
    pub aliases: Vec<LitStr>,
    pub warning: Option<ConfigWarning>,
}

pub(crate) struct ConfigWarning {
    pub kind: &'static str,
    pub message: Option<LitStr>,
}

impl ConfigAttrs {
    pub fn parse(attributes: &[Attribute]) -> syn::Result<Self> {
        let mut parsed = Self::default();

        for attribute in attributes.iter().filter(|attr| attr.path().is_ident("config")) {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    if mem::replace(&mut parsed.skip, true) {
                        return Err(meta.error("duplicate `skip` attribute"));
                    }
                    return Ok(());
                }
                if meta.path.is_ident("flatten") {
                    if mem::replace(&mut parsed.flatten, true) {
                        return Err(meta.error("duplicate `flatten` attribute"));
                    }
                    return Ok(());
                }
                if meta.path.is_ident("alias") {
                    let alias = meta.value()?.parse::<LitStr>()?;
                    if alias.value().trim().is_empty() {
                        return Err(meta.error("alias must not be empty"));
                    }
                    if parsed.aliases.iter().any(|existing| existing.value() == alias.value()) {
                        return Err(meta.error("duplicate alias"));
                    }
                    parsed.aliases.push(alias);
                    return Ok(());
                }

                let kind = if meta.path.is_ident("deprecated") {
                    "deprecated"
                } else if meta.path.is_ident("removed") {
                    "removed"
                } else {
                    return Err(meta.error("unknown config attribute"));
                };
                if parsed.warning.is_some() {
                    return Err(meta.error("only one of `deprecated` or `removed` is allowed"));
                }
                let message = if meta.input.peek(Token![=]) {
                    Some(meta.value()?.parse::<LitStr>()?)
                } else {
                    None
                };
                parsed.warning = Some(ConfigWarning { kind, message });
                Ok(())
            })?;
        }

        if parsed.skip && (parsed.flatten || !parsed.aliases.is_empty() || parsed.warning.is_some())
        {
            return Err(Error::new_spanned(
                &attributes[0],
                "`skip` cannot be combined with another config attribute",
            ));
        }
        if parsed.flatten && (!parsed.aliases.is_empty() || parsed.warning.is_some()) {
            return Err(Error::new_spanned(
                &attributes[0],
                "`flatten` cannot be combined with aliases or warnings",
            ));
        }

        Ok(parsed)
    }
}
