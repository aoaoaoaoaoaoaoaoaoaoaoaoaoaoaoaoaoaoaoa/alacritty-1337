use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{Error, Field, Generics, Ident, Type};

use crate::{ConfigAttrs, GenericsStreams, MULTIPLE_FLATTEN_ERROR, serde_replace};

/// Use this crate's name as log target.
const LOG_TARGET: &str = env!("CARGO_PKG_NAME");

pub fn derive_deserialize<T>(
    ident: Ident,
    generics: Generics,
    fields: Punctuated<Field, T>,
) -> TokenStream {
    // Create all necessary tokens for the implementation.
    let GenericsStreams { unconstrained, constrained, phantoms } =
        crate::generics_streams(&generics.params);
    let FieldStreams { flatten, match_assignments } = fields_deserializer(&fields);
    let visitor = format_ident!("{}Visitor", ident);

    // Generate deserialization impl.
    let mut tokens = quote! {
        #[derive(Default)]
        #[allow(non_snake_case)]
        struct #visitor <#unconstrained> {
            #phantoms
        }

        impl <'de, #constrained> serde::de::Visitor<'de> for #visitor <#unconstrained> {
            type Value = #ident <#unconstrained>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a mapping")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut config = Self::Value::default();

                // Unused keys for flattening and warning.
                let mut unused = toml::Table::new();

                while let Some((key, value)) = map.next_entry::<String, toml::Value>()? {
                    match key.as_str() {
                        #match_assignments
                        _ => {
                            unused.insert(key, value);
                        },
                    }
                }

                #flatten

                // Warn about unused keys.
                for key in unused.keys() {
                    log::warn!(target: #LOG_TARGET, "Unused config key: {}", key);
                }

                Ok(config)
            }
        }

        impl <'de, #constrained> serde::Deserialize<'de> for #ident <#unconstrained> {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_map(#visitor :: default())
            }
        }
    };

    // Automatically implement [`alacritty_config::SerdeReplace`].
    tokens.extend(serde_replace::derive_recursive(ident, generics, fields));

    tokens.into()
}

// Token streams created from the fields in the struct.
#[derive(Default)]
struct FieldStreams {
    match_assignments: TokenStream2,
    flatten: TokenStream2,
}

/// Create the deserializers for match arms and flattened fields.
fn fields_deserializer<T>(fields: &Punctuated<Field, T>) -> FieldStreams {
    let mut field_streams = FieldStreams::default();

    // Create the deserialization stream for each field.
    for field in fields {
        if let Err(err) = field_deserializer(&mut field_streams, field) {
            field_streams.flatten = err.to_compile_error();
            return field_streams;
        }
    }

    field_streams
}

/// Append a single field deserializer to the stream.
fn field_deserializer(field_streams: &mut FieldStreams, field: &Field) -> Result<(), Error> {
    let Some(ident) = field.ident.as_ref() else {
        return Err(Error::new_spanned(field, "ConfigDeserialize requires named fields"));
    };
    let literal = ident.to_string();
    let mut literals = vec![literal.clone()];
    let attributes = ConfigAttrs::parse(&field.attrs)?;

    if attributes.skip {
        return Ok(());
    }

    // Create default stream for deserializing fields.
    let mut match_assignment_stream = quote! {
        match serde::Deserialize::deserialize(value) {
            Ok(value) => config.#ident = value,
            Err(err) => {
                log::error!(
                    target: #LOG_TARGET,
                    "Config error: {}: {}",
                    #literal,
                    err.to_string().trim(),
                );
            },
        }
    };

    if attributes.flatten {
        // NOTE: Currently only a single instance of flatten is supported per struct.
        if !field_streams.flatten.is_empty() {
            return Err(Error::new(ident.span(), MULTIPLE_FLATTEN_ERROR));
        }

        field_streams.flatten.extend(quote! {
            let flattened = std::mem::take(&mut unused);
            config.#ident = serde::Deserialize::deserialize(flattened).unwrap_or_default();
        });
    }

    if let Some(warning) = attributes.warning {
        let mut message = format!("Config warning: {} has been {}", literal, warning.kind);
        if let Some(warning) = warning.message {
            message = format!("{}; {}", message, warning.value());
        }
        message.push_str("\nUse `alacritty migrate` to automatically resolve it");
        match_assignment_stream.extend(quote! {
            log::warn!(target: #LOG_TARGET, #message);
        });
    }

    for alias in attributes.aliases {
        literals.push(alias.value());
    }

    // Create token stream for deserializing "none" string into `Option<T>`.
    if let Type::Path(type_path) = &field.ty
        && type_path.path.segments.iter().next_back().is_some_and(|s| s.ident == "Option")
    {
        match_assignment_stream = quote! {
            if value.as_str().is_some_and(|s| s.eq_ignore_ascii_case("none")) {
                config.#ident = None;
                continue;
            }
            #match_assignment_stream
        };
    }

    // Create the token stream for deserialization and error handling.
    field_streams.match_assignments.extend(quote! {
        #(#literals)|* => { #match_assignment_stream },
    });

    Ok(())
}
