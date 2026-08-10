use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    Data, DataStruct, DeriveInput, Error, Field, Fields, Generics, Ident, LitStr, parse_macro_input,
};

use crate::{ConfigAttrs, GenericsStreams, MULTIPLE_FLATTEN_ERROR};

/// Error if the derive was used on an unsupported type.
const UNSUPPORTED_ERROR: &str = "SerdeReplace must be used on a tuple struct";

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match input.data {
        Data::Struct(DataStruct { fields: Fields::Unnamed(_), .. }) | Data::Enum(_) => {
            derive_direct(input.ident, input.generics).into()
        },
        Data::Struct(DataStruct { fields: Fields::Named(fields), .. }) => {
            derive_recursive(input.ident, input.generics, fields.named).into()
        },
        _ => Error::new(input.ident.span(), UNSUPPORTED_ERROR).to_compile_error().into(),
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "derive entrypoints own their one-shot syntax tree"
)]
pub fn derive_direct(ident: Ident, generics: Generics) -> TokenStream2 {
    quote! {
        impl <#generics> alacritty_config::SerdeReplace for #ident <#generics> {
            fn replace(&mut self, value: toml::Value) -> Result<(), Box<dyn std::error::Error>> {
                *self = serde::Deserialize::deserialize(value)?;

                Ok(())
            }
        }
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "derive entrypoints own their one-shot syntax tree"
)]
pub fn derive_recursive<T>(
    ident: Ident,
    generics: Generics,
    fields: Punctuated<Field, T>,
) -> TokenStream2 {
    let GenericsStreams { unconstrained, constrained, .. } =
        crate::generics_streams(&generics.params);
    let (replace_arms, replace_flattened) = match match_arms(&fields) {
        Err(e) => return e.to_compile_error(),
        Ok(replace_arms) => replace_arms,
    };

    quote! {
        #[allow(clippy::extra_unused_lifetimes)]
        impl <'de, #constrained> alacritty_config::SerdeReplace for #ident <#unconstrained> {
            fn replace(&mut self, value: toml::Value) -> Result<(), Box<dyn std::error::Error>> {
                match value {
                    toml::Value::Table(table) => {
                        let mut flattened = toml::Table::new();
                        for (field, next_value) in table {
                            match field.as_str() {
                                #replace_arms
                                _ => {
                                    let error = format!("Field \"{}\" does not exist", field);
                                    return Err(error.into());
                                },
                            }
                        }
                        #replace_flattened
                    },
                    value => *self = serde::Deserialize::deserialize(value)?,
                }

                Ok(())
            }
        }
    }
}

/// Create `SerdeReplace` recursive match arms.
fn match_arms<T>(fields: &Punctuated<Field, T>) -> Result<(TokenStream2, TokenStream2), Error> {
    let mut stream = TokenStream2::default();
    let mut flattened_arm = None;

    // Create arm for each field.
    for field in fields {
        let Some(ident) = field.ident.as_ref() else {
            return Err(Error::new_spanned(field, "SerdeReplace requires named fields"));
        };
        let literal = ident.to_string();
        let attributes = ConfigAttrs::parse(&field.attrs)?;

        if attributes.skip {
            continue;
        }

        if attributes.flatten && flattened_arm.is_some() {
            return Err(Error::new(ident.span(), MULTIPLE_FLATTEN_ERROR));
        } else if attributes.flatten {
            flattened_arm = Some((
                quote! {
                    _ => {
                        flattened.insert(field, next_value);
                    },
                },
                ident.clone(),
            ));
        } else {
            let aliases = attributes.aliases.iter().map(LitStr::value);

            stream.extend(quote! {
                #(#aliases)|* | #literal => {
                    alacritty_config::SerdeReplace::replace(&mut self.#ident, next_value)?
                },
            });
        }
    }

    // Add the flattened catch-all as last match arm.
    let replace_flattened = if let Some((flattened_arm, ident)) = flattened_arm.take() {
        stream.extend(flattened_arm);
        quote! {
            if !flattened.is_empty() {
                alacritty_config::SerdeReplace::replace(
                    &mut self.#ident,
                    toml::Value::Table(flattened),
                )?;
            }
        }
    } else {
        TokenStream2::new()
    };

    Ok((stream, replace_flattened))
}
