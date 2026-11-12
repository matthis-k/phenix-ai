//! Explicit-schema, length-delimited record codec generation.
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Fields, GenericArgument, LitStr, PathArguments, Type};

struct FieldSpec {
    ident: syn::Ident,
    value_type: Type,
    optional: bool,
    name: LitStr,
}

fn fields(input: &DeriveInput) -> syn::Result<Vec<FieldSpec>> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "ContractValue requires a non-generic named struct",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "derive requires a struct",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "derive requires named fields",
        ));
    };
    Ok(fields
        .named
        .iter()
        .map(|field| {
            let ident = field.ident.clone().expect("named field");
            let inner = match &field.ty {
                Type::Path(path) => path.path.segments.last().and_then(|segment| {
                    if segment.ident != "Option" {
                        return None;
                    }
                    let PathArguments::AngleBracketed(args) = &segment.arguments else {
                        return None;
                    };
                    match args.args.first()? {
                        GenericArgument::Type(ty) if args.args.len() == 1 => Some(ty.clone()),
                        _ => None,
                    }
                }),
                _ => None,
            };
            FieldSpec {
                name: LitStr::new(&ident.unraw().to_string(), ident.span()),
                ident,
                optional: inner.is_some(),
                value_type: inner.unwrap_or_else(|| field.ty.clone()),
            }
        })
        .collect())
}

pub(super) fn derive(input: DeriveInput) -> syn::Result<Tokens> {
    let ident = &input.ident;
    let fields = fields(&input)?;
    let mut schema: Option<LitStr> = None;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("contract")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("schema") {
                if schema.is_some() {
                    return Err(meta.error("duplicate contract schema"));
                }
                schema = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("supported: schema = \"stable.schema.id\""))
            }
        })?;
    }
    let schema = schema.ok_or_else(|| {
        syn::Error::new_spanned(
            ident,
            "#[derive(ContractValue)] requires #[contract(schema = \"stable.schema.id\")]",
        )
    })?;
    if schema.value().trim().is_empty() {
        return Err(syn::Error::new_spanned(&schema, "schema must not be empty"));
    }
    let write_fields = fields.iter().map(|f| {
        let ident = &f.ident;
        if f.optional {
            // Write the tag directly into the output, not into a second payload Vec.
            quote! {
                match &self.#ident {
                    None => {
                        out.extend_from_slice(&1u64.to_le_bytes());
                        out.push(0);
                    }
                    Some(value) => {
                        let field = ::phenix_graph::ContractValue::encode(value);
                        let length = u64::try_from(field.len()).expect("field too large")
                            .checked_add(1).expect("field too large");
                        out.extend_from_slice(&length.to_le_bytes());
                        out.push(1);
                        out.extend_from_slice(&field);
                    }
                }
            }
        } else {
            quote! {
                let field = ::phenix_graph::ContractValue::encode(&self.#ident);
                out.extend_from_slice(&u64::try_from(field.len()).expect("field too large").to_le_bytes());
                out.extend_from_slice(&field);
            }
        }
    });
    let read_fields = fields.iter().map(|f| {
        let ident = &f.ident;
        let name = &f.name;
        let ty = &f.value_type;
        let decoder = if f.optional {
            quote! {
                match field.split_first() {
                    Some((&0, rest)) if rest.is_empty() => None,
                    Some((&1, rest)) => Some(<#ty as ::phenix_graph::ContractValue>::decode(rest)?),
                    _ => return Err(::std::format!("invalid Option encoding for {}", #name)),
                }
            }
        } else {
            quote! { <#ty as ::phenix_graph::ContractValue>::decode(field)? }
        };
        // Field identifiers appear only as members, never as decoder locals.
        quote! {
            #ident: {
                let header_end = cursor.checked_add(8).ok_or("field offset overflow")?;
                let header = bytes.get(cursor..header_end).ok_or("truncated field header")?;
                let length = usize::try_from(u64::from_le_bytes(
                    header.try_into().map_err(|_| "invalid field header")?
                )).map_err(|_| "field length overflow")?;
                let end = header_end.checked_add(length).ok_or("field length overflow")?;
                let field = bytes.get(header_end..end).ok_or("truncated field payload")?;
                let value = #decoder;
                cursor = end;
                value
            }
        }
    });
    let descriptors = fields.iter().map(|f| {
        let name = &f.name;
        let ty = &f.value_type;
        let optional = f.optional;
        quote! { (#name, #optional, <#ty as ::phenix_graph::ContractValue>::schema()) }
    });
    Ok(quote! {
        impl ::phenix_graph::ContractValue for #ident {
            fn schema() -> ::phenix_graph::Schema {
                ::phenix_graph::Schema::record(#schema, &[#(#descriptors),*])
            }
            fn encode(&self) -> ::std::vec::Vec<u8> {
                let mut out = ::std::vec::Vec::new();
                #(#write_fields)*
                out
            }
            fn decode(bytes: &[u8]) -> ::std::result::Result<Self, ::std::string::String> {
                let mut cursor = 0usize;
                let value = Self { #(#read_fields),* };
                if cursor != bytes.len() { return Err("trailing bytes in contract value".into()); }
                Ok(value)
            }
        }
    })
}
