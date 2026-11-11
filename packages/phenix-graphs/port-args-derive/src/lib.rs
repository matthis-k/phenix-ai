//! Syntax sugar for canonical graph interfaces and explicit-schema record codecs.
use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use std::collections::BTreeSet;
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

mod contract_value;

fn derive_ports(input: DeriveInput) -> syn::Result<Tokens> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Ports requires a named struct",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Ports requires named fields (use struct Empty {} for no ports)",
        ));
    };
    let mut names = BTreeSet::new();
    let mut keys = Vec::new();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut bounds = Vec::new();
    let mut record_bounds = Vec::new();
    let mut record_fields = Vec::new();
    let mut record_steps = Vec::new();
    let mut paths = BTreeSet::new();
    let mut input_only = true;
    for field in &fields.named {
        let ident = field.ident.as_ref().expect("named field");
        let mut name = ident.unraw().to_string();
        let upper = name.to_uppercase();
        if !names.insert(upper.clone()) {
            return Err(syn::Error::new_spanned(
                ident,
                format!("generated port constant {upper} collides with another field"),
            ));
        }
        let constant = syn::parse_str::<syn::Ident>(&upper).map_err(|_| {
            syn::Error::new_spanned(
                ident,
                "field does not produce a valid uppercase port constant",
            )
        })?;
        let mut direction = None;
        let mut encoded = false;
        let mut flatten = false;
        let mut renamed = false;
        for attr in &field.attrs {
            let is_input = attr.path().is_ident("input");
            if !is_input && !attr.path().is_ident("output") {
                continue;
            }
            if direction.replace(is_input).is_some() {
                return Err(syn::Error::new_spanned(
                    attr,
                    "each field requires exactly one #[input] or #[output]",
                ));
            }
            if !matches!(attr.meta, syn::Meta::Path(_)) {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("name") && !renamed {
                        let literal: syn::LitStr = meta.value()?.parse()?;
                        name = literal.value();
                        if name.split('.').any(|part| part.trim().is_empty())
                            || name.chars().any(char::is_control)
                        {
                            return Err(syn::Error::new_spanned(
                                literal,
                                "port path must contain nonempty, control-free segments",
                            ));
                        }
                        renamed = true;
                    } else if meta.path.is_ident("flatten") && !flatten {
                        flatten = true;
                    } else if meta.path.is_ident("encoded") && !encoded {
                        encoded = true;
                    } else {
                        return Err(meta.error(
                            "supported: a single encoded directive, flatten, name = \"path\"",
                        ));
                    }
                    Ok(())
                })?;
                if !encoded && !flatten && !renamed {
                    return Err(syn::Error::new_spanned(attr, "expected encoded directive"));
                }
            }
        }
        let is_input = direction.ok_or_else(|| {
            syn::Error::new_spanned(
                field,
                "each field requires exactly one #[input] or #[output]",
            )
        })?;
        input_only &= is_input;
        if flatten && (!is_input || encoded) {
            return Err(syn::Error::new_spanned(
                field,
                "flatten requires an input record; encoded mode belongs on its leaves",
            ));
        }
        if !paths.insert((is_input, name.clone())) {
            return Err(syn::Error::new_spanned(
                field,
                format!("duplicate port path {name:?}"),
            ));
        }
        let ty = &field.ty;
        let binding = syn::Ident::new(
            &format!("__phenix_field_{}", record_fields.len()),
            ident.span(),
        );
        record_fields.push(quote!(#ident: #binding));
        if flatten {
            bounds.push(quote!(#ty: ::phenix_graph::InputRecord));
            keys.push(quote! {
                pub const #constant: ::phenix_graph::InputGroup<#ty> = ::phenix_graph::InputGroup::new(#name);
            });
            inputs.push(quote! { ports.extend(Self::#constant.ports()?); });
            record_steps.push((binding, quote!(Self::#constant.map_inputs)));
            continue;
        }
        let key = if is_input {
            quote!(::phenix_graph::InputKey)
        } else {
            quote!(::phenix_graph::OutputKey)
        };
        let constructor = if encoded {
            bounds.push(quote!(#ty: ::phenix_graph::ContractValue));
            quote!(encoded)
        } else {
            bounds.push(quote!(#ty: 'static));
            quote!(new)
        };
        keys.push(quote! {
            pub const #constant: #key<#ty> = #key::#constructor(#name);
        });
        if is_input {
            inputs.push(quote! { ports.push(Self::#constant.definition()?); });
            record_bounds.push(quote!(#ty: ::std::marker::Send + ::std::marker::Sync));
            let shared = matches!(ty, syn::Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "Arc"));
            let method = if shared && !encoded {
                quote!(map_shared_input)
            } else {
                quote!(map_input)
            };
            record_steps.push((binding, quote!(Self::#constant.#method)));
        } else {
            outputs.push(quote!(Self::#constant.definition()?));
        }
    }
    let ident = &input.ident;
    let mut generics = input.generics.clone();
    for bound in bounds {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse2(bound)?);
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let record_impl = if input_only {
        let mut record_generics = generics.clone();
        for bound in record_bounds {
            record_generics
                .make_where_clause()
                .predicates
                .push(syn::parse2(bound)?);
        }
        let (record_impl_generics, _, record_where) = record_generics.split_for_impl();
        let mut assembly = quote! { finish(context, Self { #(#record_fields),* }) };
        for (binding, step) in record_steps.into_iter().rev() {
            // Close each leaf continuation before attaching that leaf's leases.
            assembly = quote! { #step(context, prefix, |context, #binding| { #assembly }) };
        }
        // A prefix is applied to the same canonical metadata used by interface().
        Some(quote! {
            impl #record_impl_generics ::phenix_graph::InputRecord for #ident #ty_generics #record_where {
                fn input_ports() -> ::std::result::Result<::std::vec::Vec<::phenix_graph::PortDef>, ::phenix_graph::InterfaceError> {
                    let mut ports = ::std::vec::Vec::new();
                    #(#inputs)*
                    Ok(ports)
                }
                fn map_inputs(
                    context: &mut ::phenix_graph::ProcessContext,
                    prefix: &str,
                    finish: impl FnOnce(&mut ::phenix_graph::ProcessContext, Self) -> ::std::result::Result<::phenix_graph::Value, ::std::string::String>,
                ) -> ::std::result::Result<::phenix_graph::Value, ::std::string::String> {
                    #assembly
                }
            }
        })
    } else {
        None
    };
    Ok(quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            #(#keys)*
            pub fn interface() -> ::std::result::Result<::std::sync::Arc<::phenix_graph::Interface>, ::phenix_graph::InterfaceError> {
                let mut ports = ::std::vec::Vec::new();
                #(#inputs)*
                ::std::result::Result::Ok(::std::sync::Arc::new(::phenix_graph::Interface::new(
                    ports, [#(#outputs),*]
                )?))
            }
        }
        #record_impl
    })
}

/// Declare named local ports, or portable ports with `#[input(encoded)]` and
/// `#[output(encoded)]`. Generates uppercase typed keys and `Type::interface()`.
/// `name = "request.value"` renames a leaf; `#[input(flatten)]` expands an
/// input-only record into prefixed leaf ports and a composable InputGroup.
/// Input-only structs also implement InputRecord for scoped reconstruction.
#[proc_macro_derive(Ports, attributes(input, output))]
pub fn ports(input: TokenStream) -> TokenStream {
    derive_ports(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(ContractValue, attributes(contract, slot))]
pub fn contract_value(input: TokenStream) -> TokenStream {
    contract_value::derive(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_and_collisions_are_diagnosed() {
        for (input, expected) in [
            (
                syn::parse_quote!(
                    struct Missing {
                        value: i64,
                    }
                ),
                "exactly one",
            ),
            (
                syn::parse_quote!(
                    struct Both {
                        #[input]
                        #[output]
                        value: i64,
                    }
                ),
                "exactly one",
            ),
            (
                syn::parse_quote!(
                    struct Duplicate {
                        #[input]
                        #[input]
                        value: i64,
                    }
                ),
                "exactly one",
            ),
            (
                syn::parse_quote!(
                    struct Collision {
                        #[input]
                        foo: i64,
                        #[output]
                        FOO: i64,
                    }
                ),
                "constant FOO collides",
            ),
            (
                syn::parse_quote!(
                    struct Bad {
                        #[input(other)]
                        value: i64,
                    }
                ),
                "single encoded",
            ),
            (
                syn::parse_quote!(
                    struct Bad {
                        #[input()]
                        value: i64,
                    }
                ),
                "expected encoded",
            ),
            (
                syn::parse_quote!(
                    struct Tuple(i64);
                ),
                "named fields",
            ),
            (
                syn::parse_quote!(
                    enum Bad {}
                ),
                "named struct",
            ),
            (
                syn::parse_quote!(
                    struct Duplicate {
                        #[input(name = "request.value")]
                        a: i64,
                        #[input(name = "request.value")]
                        b: i64,
                    }
                ),
                "duplicate port path",
            ),
            (
                syn::parse_quote!(
                    struct Bad {
                        #[input(name = "request..value")]
                        a: i64,
                    }
                ),
                "nonempty, control-free",
            ),
            (
                syn::parse_quote!(
                    struct Bad {
                        #[output(flatten)]
                        a: i64,
                    }
                ),
                "flatten requires an input record",
            ),
            (
                syn::parse_quote!(
                    struct Bad {
                        #[input(flatten, encoded)]
                        a: i64,
                    }
                ),
                "encoded mode belongs on its leaves",
            ),
            (
                syn::parse_quote!(
                    struct Bad {
                        #[input(name = "a", name = "b")]
                        a: i64,
                    }
                ),
                "single encoded directive",
            ),
        ] {
            assert!(
                derive_ports(input)
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
        }
    }

    #[test]
    fn raw_generic_encoded_and_empty_structs_expand_to_rust() {
        for input in [
            syn::parse_quote!(
                struct Raw<T>
                where
                    T: Clone,
                {
                    #[input]
                    r#type: T,
                    #[output(encoded)]
                    result: i64,
                }
            ),
            syn::parse_quote!(
                struct Empty {}
            ),
            syn::parse_quote!(
                struct Nested<T> {
                    #[input(flatten, name = "request")]
                    r#type: T,
                }
            ),
        ] {
            syn::parse2::<syn::File>(derive_ports(input).unwrap()).unwrap();
        }
    }

    #[test]
    fn codec_requires_one_explicit_nonempty_schema() {
        for (input, message) in [
            (
                syn::parse_quote!(
                    struct Missing {
                        value: i64,
                    }
                ),
                "requires #[contract",
            ),
            (
                syn::parse_quote!(
                    #[contract(schema = " ")]
                    struct Empty {
                        value: i64,
                    }
                ),
                "schema must not be empty",
            ),
            (
                syn::parse_quote!(
                    #[contract(schema = "a", schema = "b")]
                    struct Duplicate {
                        value: i64,
                    }
                ),
                "duplicate contract schema",
            ),
        ] {
            assert!(
                contract_value::derive(input)
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
        }
    }
}
