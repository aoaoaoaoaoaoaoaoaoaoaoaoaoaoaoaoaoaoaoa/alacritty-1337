use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{DataEnum, Generics, Ident};

use crate::{ConfigAttrs, serde_replace};

pub fn derive_deserialize(ident: Ident, generics: Generics, data_enum: DataEnum) -> TokenStream {
    let visitor = format_ident!("{}Visitor", ident);

    // Create match arm streams and get a list with all available values.
    let mut match_arms_stream = TokenStream2::new();
    let mut available_values = String::from("one of ");
    for variant in &data_enum.variants {
        let attributes = match ConfigAttrs::parse(&variant.attrs) {
            Ok(attributes) => attributes,
            Err(err) => return err.to_compile_error().into(),
        };
        if attributes.flatten || !attributes.aliases.is_empty() || attributes.warning.is_some() {
            return syn::Error::new_spanned(
                variant,
                "enum variants support only `#[config(skip)]`",
            )
            .to_compile_error()
            .into();
        }
        if attributes.skip {
            continue;
        }

        let variant_ident = &variant.ident;
        let variant_str = variant_ident.to_string();
        available_values = format!("{available_values}`{variant_str}`, ");

        let literal = variant_str.to_lowercase();

        match_arms_stream.extend(quote! {
            #literal => Ok(#ident :: #variant_ident),
        });
    }

    // Remove trailing `, ` from the last enum variant.
    available_values.truncate(available_values.len().saturating_sub(2));

    // Generate deserialization impl.
    let mut tokens = quote! {
        struct #visitor;
        impl<'de> serde::de::Visitor<'de> for #visitor {
            type Value = #ident;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(#available_values)
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match s.to_lowercase().as_str() {
                    #match_arms_stream
                    _ => Err(E::custom(
                            &format!("unknown variant `{}`, expected {}", s, #available_values)
                    )),
                }
            }
        }

        impl<'de> serde::Deserialize<'de> for #ident {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_str(#visitor)
            }
        }
    };

    // Automatically implement [`alacritty_config::SerdeReplace`].
    tokens.extend(serde_replace::derive_direct(ident, generics));

    tokens.into()
}
