#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use quote::quote;
use syn::ext::IdentExt;
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Fields, Generics, Ident, LitStr, Type,
};

mod component_attr;
mod component_runtime_attr;
mod expose_attr;
mod interface_attr;
mod plugin_attr;
mod resource_attr;

#[proc_macro_attribute]
pub fn component(args: TokenStream, input: TokenStream) -> TokenStream {
    component_runtime_attr::expand(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn expose(args: TokenStream, input: TokenStream) -> TokenStream {
    expose_attr::expand(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn interface(args: TokenStream, input: TokenStream) -> TokenStream {
    interface_attr::expand(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn plugin(args: TokenStream, input: TokenStream) -> TokenStream {
    plugin_attr::expand(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn resource(args: TokenStream, input: TokenStream) -> TokenStream {
    resource_attr::expand(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(PhenixValue, attributes(phenix))]
pub fn derive_phenix_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_value(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(PhenixContract, attributes(phenix))]
pub fn derive_phenix_contract(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_contract(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[derive(Default)]
struct PhenixAttributes {
    id: Option<LitStr>,
}

fn derive_value(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    phenix_attributes(&input)?;
    let project_from_value = derive_project_value(&input.data)?;
    let name = input.ident;
    let generics = with_value_bounds(input.generics);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let (phenix_type, to_value, from_value) = match input.data {
        Data::Struct(data) => derive_struct(&data.fields)?,
        Data::Enum(data) => derive_enum(&data.variants.iter().collect::<Vec<_>>())?,
        Data::Union(data) => {
            return Err(syn::Error::new_spanned(
                data.union_token,
                "PhenixValue cannot be derived for unions",
            ))
        }
    };

    Ok(quote! {
        impl #impl_generics ::phenix_core::ValueCodec for #name #type_generics #where_clause {
            fn phenix_type() -> ::phenix_core::PhenixSchema {
                #phenix_type
            }

            fn to_value(&self) -> ::phenix_core::PhenixValue {
                #to_value
            }

            fn from_value(value: &::phenix_core::PhenixValue) -> ::std::result::Result<Self, ::phenix_core::ValueError> {
                <Self as ::phenix_core::ValueCodec>::phenix_type().parse(value)?;
                #from_value
            }

            fn project_from_value(value: &::phenix_core::PhenixValue) -> ::std::result::Result<Self, ::phenix_core::ValueError> {
                #project_from_value
            }
        }

        impl #impl_generics ::std::convert::From<&#name #type_generics> for ::phenix_core::PhenixValue #where_clause {
            fn from(value: &#name #type_generics) -> ::phenix_core::PhenixValue {
                <#name #type_generics as ::phenix_core::ValueCodec>::to_value(value)
            }
        }

        impl #impl_generics ::std::convert::TryFrom<&::phenix_core::PhenixValue> for #name #type_generics #where_clause {
            type Error = ::phenix_core::ValueError;

            fn try_from(value: &::phenix_core::PhenixValue) -> ::std::result::Result<Self, ::phenix_core::ValueError> {
                <Self as ::phenix_core::ValueCodec>::project_from_value(value)
            }
        }

        impl #impl_generics ::std::convert::TryFrom<::phenix_core::Exact<&::phenix_core::PhenixValue>> for #name #type_generics #where_clause {
            type Error = ::phenix_core::ValueError;

            fn try_from(value: ::phenix_core::Exact<&::phenix_core::PhenixValue>) -> ::std::result::Result<Self, ::phenix_core::ValueError> {
                <Self as ::phenix_core::ValueCodec>::from_value(value.0)
            }
        }

        impl #impl_generics ::std::convert::TryFrom<::phenix_core::Project<&::phenix_core::PhenixValue>> for #name #type_generics #where_clause {
            type Error = ::phenix_core::ValueError;

            fn try_from(value: ::phenix_core::Project<&::phenix_core::PhenixValue>) -> ::std::result::Result<Self, ::phenix_core::ValueError> {
                <Self as ::phenix_core::ValueCodec>::project_from_value(value.0)
            }
        }
    })
}

fn derive_contract(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "PhenixContract cannot be generic because one contract id must describe one structural shape",
        ));
    }

    let attributes = phenix_attributes(&input)?;
    let id = attributes.id.ok_or_else(|| {
        syn::Error::new_spanned(
            &input.ident,
            "PhenixContract requires #[phenix(id = \"name@version\")]",
        )
    })?;
    validate_contract_id(&id.value()).map_err(|error| syn::Error::new_spanned(&id, error))?;
    let name = input.ident;

    Ok(quote! {
        impl ::phenix_core::PhenixContract for #name {
            fn contract_id() -> ::phenix_core::ContractId {
                ::phenix_core::ContractId::parse(#id)
                    .expect("PhenixContract derive parsed the static contract id")
            }
        }
    })
}

fn validate_contract_id(value: &str) -> Result<(), &'static str> {
    let (identity, version) = value
        .rsplit_once('@')
        .ok_or("contract id must include an @version suffix")?;
    if identity.is_empty() || identity.contains('@') {
        return Err("contract id must contain one non-empty identity and @version suffix");
    }
    if !identity.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
    }) {
        return Err("contract id contains unsupported characters");
    }
    let version = version
        .parse::<u64>()
        .map_err(|_| "contract id version must be a positive integer")?;
    if version == 0 {
        return Err("contract id version must be a positive integer");
    }
    Ok(())
}

fn with_value_bounds(mut generics: Generics) -> Generics {
    for parameter in generics.type_params_mut() {
        parameter
            .bounds
            .push(parse_quote!(::phenix_core::ValueCodec));
    }
    generics
}

fn derive_project_value(data: &Data) -> syn::Result<proc_macro2::TokenStream> {
    match data {
        Data::Struct(data) => derive_project_struct(&data.fields),
        Data::Enum(data) => derive_project_enum(&data.variants.iter().collect::<Vec<_>>()),
        Data::Union(data) => Err(syn::Error::new_spanned(
            data.union_token,
            "PhenixValue cannot be derived for unions",
        )),
    }
}

fn derive_project_struct(fields: &Fields) -> syn::Result<proc_macro2::TokenStream> {
    match fields {
        Fields::Named(fields) => {
            let fields = fields
                .named
                .iter()
                .map(|field| {
                    (
                        field.ident.clone().expect("named field has an identifier"),
                        field.ty.clone(),
                    )
                })
                .collect::<Vec<_>>();
            Ok(project_named_struct(&fields, quote!(Self)))
        }
        Fields::Unit => Ok(quote! {
            <Self as ::phenix_core::ValueCodec>::from_value(value)
        }),
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let ty = &fields.unnamed[0].ty;
            Ok(quote! {
                Ok(Self(<#ty as ::phenix_core::ValueCodec>::project_from_value(value)?))
            })
        }
        Fields::Unnamed(fields) => Err(syn::Error::new_spanned(
            fields,
            "PhenixValue tuple structs must be newtypes; use a named struct for multiple fields",
        )),
    }
}

fn project_named_struct(
    fields: &[(Ident, Type)],
    constructor: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let decode_fields = fields.iter().map(|(field, ty)| {
        let key = field.unraw().to_string();
        quote! {
            #field: <#ty as ::phenix_core::ValueCodec>::project_from_value(value.get(#key)?)?
        }
    });

    quote! {
        if !matches!(value, ::phenix_core::PhenixValue::Table(_)) {
            return Err(::phenix_core::ValueError::TypeMismatch {
                expected: ::phenix_core::TypeKind::Table,
                actual: value.kind(),
            });
        }
        Ok(#constructor { #(#decode_fields),* })
    }
}

fn derive_project_enum(variants: &[&syn::Variant]) -> syn::Result<proc_macro2::TokenStream> {
    let mut decode_arms = Vec::new();

    for variant in variants {
        let variant_ident = &variant.ident;
        let tag = variant_ident.unraw().to_string();
        match &variant.fields {
            Fields::Unit => {
                decode_arms.push(quote! {
                    #tag => {
                        <() as ::phenix_core::ValueCodec>::project_from_value(payload)?;
                        Ok(Self::#variant_ident)
                    }
                });
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0].ty;
                decode_arms.push(quote! {
                    #tag => Ok(Self::#variant_ident(
                        <#ty as ::phenix_core::ValueCodec>::project_from_value(payload)?
                    ))
                });
            }
            Fields::Named(fields) => {
                let fields = fields
                    .named
                    .iter()
                    .map(|field| {
                        (
                            field.ident.clone().expect("named field has an identifier"),
                            field.ty.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let decode_fields = fields.iter().map(|(field, ty)| {
                    let name = field.unraw().to_string();
                    quote! {
                        #field: <#ty as ::phenix_core::ValueCodec>::project_from_value(payload.get(#name)?)?
                    }
                });
                decode_arms.push(quote! {
                    #tag => {
                        if !matches!(payload, ::phenix_core::PhenixValue::Table(_)) {
                            return Err(::phenix_core::ValueError::TypeMismatch {
                                expected: ::phenix_core::TypeKind::Table,
                                actual: payload.kind(),
                            });
                        }
                        Ok(Self::#variant_ident { #(#decode_fields),* })
                    }
                });
            }
            Fields::Unnamed(fields) => {
                return Err(syn::Error::new_spanned(
                    fields,
                    "PhenixValue enum tuple variants must contain one value; use a named payload struct",
                ));
            }
        }
    }

    Ok(quote! {
        let (tag, payload) = value.variant()?;
        match tag.as_str() {
            #(#decode_arms),*,
            _ => Err(::phenix_core::ValueError::unknown_variant(tag.clone())),
        }
    })
}

fn derive_struct(
    fields: &Fields,
) -> syn::Result<(
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
)> {
    match fields {
        Fields::Named(fields) => {
            let fields = fields
                .named
                .iter()
                .map(|field| {
                    (
                        field.ident.clone().expect("named field has an identifier"),
                        field.ty.clone(),
                    )
                })
                .collect::<Vec<_>>();
            Ok(named_struct(&fields, quote!(Self)))
        }
        Fields::Unit => Ok((
            quote!(::phenix_core::PhenixSchema::Unit),
            quote!(::phenix_core::PhenixValue::Unit),
            quote! {
                <() as ::phenix_core::ValueCodec>::from_value(value)?;
                Ok(Self)
            },
        )),
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let ty = &fields.unnamed[0].ty;
            Ok((
                quote!(<#ty as ::phenix_core::ValueCodec>::phenix_type()),
                quote!(::phenix_core::ValueCodec::to_value(&self.0)),
                quote!(Ok(Self(<#ty as ::phenix_core::ValueCodec>::from_value(value)?))),
            ))
        }
        Fields::Unnamed(fields) => Err(syn::Error::new_spanned(
            fields,
            "PhenixValue tuple structs must be newtypes; use a named struct for multiple fields",
        )),
    }
}

fn named_struct(
    fields: &[(Ident, Type)],
    constructor: proc_macro2::TokenStream,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    let type_fields = fields.iter().map(|(field, ty)| {
        let key = field.unraw().to_string();
        quote! {
            fields.insert(
                ::phenix_core::Key::parse(#key).expect("Rust field name is a valid structural key"),
                <#ty as ::phenix_core::ValueCodec>::phenix_type(),
            );
        }
    });
    let value_fields = fields.iter().map(|(field, _)| {
        let key = field.unraw().to_string();
        quote! {
            fields.insert(
                ::phenix_core::Key::parse(#key).expect("Rust field name is a valid structural key"),
                ::phenix_core::ValueCodec::to_value(&self.#field),
            );
        }
    });
    let decode_fields = fields.iter().map(|(field, ty)| {
        let key = field.unraw().to_string();
        quote! {
            #field: <#ty as ::phenix_core::ValueCodec>::from_value(value.get(#key)?)?
        }
    });

    (
        quote! {{
            let mut fields = ::std::collections::BTreeMap::new();
            #(#type_fields)*
            ::phenix_core::PhenixSchema::Table(fields)
        }},
        quote! {{
            let mut fields = ::std::collections::BTreeMap::new();
            #(#value_fields)*
            ::phenix_core::PhenixValue::Table(fields)
        }},
        quote! {
            Ok(#constructor { #(#decode_fields),* })
        },
    )
}

fn derive_enum(
    variants: &[&syn::Variant],
) -> syn::Result<(
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
)> {
    let mut type_arms = Vec::new();
    let mut encode_arms = Vec::new();
    let mut decode_arms = Vec::new();

    for variant in variants {
        let variant_ident = &variant.ident;
        let tag = variant_ident.unraw().to_string();
        let key = quote!(::phenix_core::Key::parse(#tag).expect("Rust variant name is a valid structural key"));

        match &variant.fields {
            Fields::Unit => {
                type_arms.push(quote! {
                    variants.insert(#key, ::phenix_core::PhenixSchema::Unit);
                });
                encode_arms.push(quote! {
                    Self::#variant_ident => ::phenix_core::PhenixValue::Variant {
                        tag: #key,
                        value: Box::new(::phenix_core::PhenixValue::Unit),
                    }
                });
                decode_arms.push(quote! {
                    #tag => {
                        <() as ::phenix_core::ValueCodec>::from_value(payload)?;
                        Ok(Self::#variant_ident)
                    }
                });
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0].ty;
                type_arms.push(quote! {
                    variants.insert(#key, <#ty as ::phenix_core::ValueCodec>::phenix_type());
                });
                encode_arms.push(quote! {
                    Self::#variant_ident(value) => ::phenix_core::PhenixValue::Variant {
                        tag: #key,
                        value: Box::new(::phenix_core::ValueCodec::to_value(value)),
                    }
                });
                decode_arms.push(quote! {
                    #tag => Ok(Self::#variant_ident(<#ty as ::phenix_core::ValueCodec>::from_value(payload)?))
                });
            }
            Fields::Named(fields) => {
                let fields = fields
                    .named
                    .iter()
                    .map(|field| {
                        (
                            field.ident.clone().expect("named field has an identifier"),
                            field.ty.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let type_fields = fields.iter().map(|(field, ty)| {
                    let name = field.unraw().to_string();
                    quote! {
                        (
                            ::phenix_core::Key::parse(#name).expect("Rust field name is a valid structural key"),
                            <#ty as ::phenix_core::ValueCodec>::phenix_type(),
                        )
                    }
                });
                let pattern_fields = fields.iter().map(|(field, _)| field);
                let value_fields = fields.iter().map(|(field, _)| {
                    let name = field.unraw().to_string();
                    quote! {
                        (
                            ::phenix_core::Key::parse(#name).expect("Rust field name is a valid structural key"),
                            ::phenix_core::ValueCodec::to_value(#field),
                        )
                    }
                });
                let decode_fields = fields.iter().map(|(field, ty)| {
                    let name = field.unraw().to_string();
                    quote! {
                        #field: <#ty as ::phenix_core::ValueCodec>::from_value(payload.get(#name)?)?
                    }
                });
                type_arms.push(quote! {
                    variants.insert(
                        #key,
                        ::phenix_core::PhenixSchema::Table(::std::collections::BTreeMap::from([
                            #(#type_fields),*
                        ])),
                    );
                });
                encode_arms.push(quote! {
                    Self::#variant_ident { #(#pattern_fields),* } => {
                        ::phenix_core::PhenixValue::Variant {
                            tag: #key,
                            value: Box::new(::phenix_core::PhenixValue::Table(
                                ::std::collections::BTreeMap::from([
                                    #(#value_fields),*
                                ]),
                            )),
                        }
                    }
                });
                decode_arms.push(quote! {
                    #tag => Ok(Self::#variant_ident { #(#decode_fields),* })
                });
            }
            Fields::Unnamed(fields) => {
                return Err(syn::Error::new_spanned(
                    fields,
                    "PhenixValue enum tuple variants must contain one value; use a named payload struct",
                ));
            }
        }
    }

    Ok((
        quote! {{
            let mut variants = ::std::collections::BTreeMap::new();
            #(#type_arms)*
            ::phenix_core::PhenixSchema::Variant(variants)
        }},
        quote! {
            match self {
                #(#encode_arms),*
            }
        },
        quote! {
            let (tag, payload) = value.variant()?;
            match tag.as_str() {
                #(#decode_arms),*,
                _ => Err(::phenix_core::ValueError::unknown_variant(tag.clone())),
            }
        },
    ))
}

fn phenix_attributes(input: &DeriveInput) -> syn::Result<PhenixAttributes> {
    let mut attributes = PhenixAttributes::default();
    for attribute in &input.attrs {
        if !attribute.path().is_ident("phenix") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                if attributes.id.is_some() {
                    return Err(meta.error("duplicate phenix id"));
                }
                attributes.id = Some(meta.value()?.parse::<LitStr>()?);
                return Ok(());
            }
            Err(meta.error("unsupported phenix attribute"))
        })?;
    }
    Ok(attributes)
}
