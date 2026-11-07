//! Optional syntax reduction for the graph prototype. Both derives generate
//! implementations of ordinary, manually implementable library traits.
//! No generated logic is required by graph compilation or scheduling.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{quote, ToTokens};
use std::collections::BTreeSet;
use syn::{parse_macro_input, Data, DeriveInput, Fields, GenericArgument, LitInt, LitStr, PathArguments, Type};

struct FieldSpec {
    ident: syn::Ident,
    value_type: Type,
    optional: bool,
    name: LitStr,
    slot_id: u16,
    immediate: bool,
}

fn option_inner(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else { return None; };
    let seg = path.path.segments.last()?;
    if seg.ident != "Option" { return None; }
    let PathArguments::AngleBracketed(args) = &seg.arguments else { return None; };
    if args.args.len() != 1 { return None; }
    match args.args.first()? {
        GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    }
}

fn fields(input: &DeriveInput, parse_slots: bool) -> syn::Result<Vec<FieldSpec>> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&input.ident, "prototype derive supports only non-generic named structs; implement the trait manually for generic types"));
    }
    let Data::Struct(s) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "derive requires a struct"));
    };
    let Fields::Named(named) = &s.fields else {
        return Err(syn::Error::new_spanned(&input.ident, "derive requires named fields"));
    };
    let mut result = Vec::new();
    let mut seen_ids = BTreeSet::new();
    let mut seen_names = BTreeSet::new();
    for (index, field) in named.named.iter().enumerate() {
        let ident = field.ident.clone().expect("named field");
        let mut slot_id = u16::try_from(index).map_err(|_| syn::Error::new_spanned(&ident, "too many fields for slot ID"))?;
        let mut name = LitStr::new(&ident.to_string(), ident.span());
        let mut immediate = false;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("slot")) {
            if !parse_slots {
                // #[derive(ContractValue, PortArgs)] is valid. Slot annotations
                // are owned exclusively by PortArgs, not the codec derive.
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("id") {
                    let number: LitInt = meta.value()?.parse()?;
                    slot_id = number.base10_parse::<u16>()?;
                    Ok(())
                } else if meta.path.is_ident("name") {
                    name = meta.value()?.parse()?;
                    Ok(())
                } else if meta.path.is_ident("buffer") {
                    let mode: LitStr = meta.value()?.parse()?;
                    match mode.value().as_str() {
                        "immediate" => immediate = true,
                        "deferred" => immediate = false,
                        _ => return Err(syn::Error::new_spanned(&mode, "buffer must be immediate or deferred")),
                    }
                    Ok(())
                } else {
                    Err(meta.error("supported: id = N, name = \"...\", buffer = \"immediate|deferred\""))
                }
            })?;
        }
        if !seen_ids.insert(slot_id) {
            return Err(syn::Error::new_spanned(&ident, format!("duplicate slot ID {slot_id}")));
        }
        if !seen_names.insert(name.value()) {
            return Err(syn::Error::new_spanned(&ident, format!("duplicate slot name {:?}", name.value())));
        }
        let optional = option_inner(&field.ty);
        result.push(FieldSpec { ident, value_type: optional.clone().unwrap_or_else(|| field.ty.clone()),
            optional: optional.is_some(), name, slot_id, immediate });
    }
    Ok(result)
}

fn derive_port_args(input: DeriveInput) -> syn::Result<Tokens> {
    let ident = &input.ident;
    let fields = fields(&input, true)?;
    let slot_exprs = fields.iter().map(|f| {
        let ty = &f.value_type;
        let id = f.slot_id;
        let name = &f.name;
        if f.immediate {
            quote! { ::phenix_graph::InputSlot::immediate::<#ty>(#id).named(#name) }
        } else {
            quote! { ::phenix_graph::InputSlot::deferred::<#ty>(#id).named(#name) }
        }
    });
    let field_exprs = fields.iter().map(|f| {
        let ident = &f.ident;
        let name = &f.name;
        let ty = &f.value_type;
        if f.optional {
            quote! { #ident: selected.optional::<#ty>(#name)? }
        } else {
            quote! { #ident: selected.get::<#ty>(#name)? }
        }
    });
    Ok(quote! {
        impl ::phenix_graph::InputSchema for #ident {
            fn slots() -> ::std::result::Result<::std::vec::Vec<::phenix_graph::InputSlot>, ::phenix_graph::LayoutError> {
                Ok(::std::vec![#(#slot_exprs),*])
            }
        }
        impl ::phenix_graph::FromSelected for #ident {
            fn from_values(selected: &::phenix_graph::SelectedValues<'_>)
                -> ::std::result::Result<Self, ::std::string::String>
            {
                Ok(Self { #(#field_exprs),* })
            }
        }
    })
}

fn scalar_field(ty: &Type) -> bool {
    if matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty()) {
        return true;
    }
    if matches!(ty, Type::Reference(_) | Type::Ptr(_) | Type::TraitObject(_) | Type::ImplTrait(_)) {
        return true;
    }
    let Type::Path(path) = ty else { return false; };
    let Some(segment) = path.path.segments.last() else { return false; };
    matches!(segment.ident.to_string().as_str(),
        "bool" | "f32" | "f64" | "i8" | "i16" | "i32" | "i64" | "i128" |
        "u8" | "u16" | "u32" | "u64" | "u128" | "String" |
        "Option" | "Vec" | "Box" | "Arc" | "Rc" | "HashMap" | "BTreeMap")
}
fn tuple_field(ty: &Type) -> bool { matches!(ty, Type::Tuple(tuple) if !tuple.elems.is_empty()) }
/// `Arc<T>` leaf fields have an explicit shared-ownership reconstruction path.
fn arc_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else { return None; };
    let segment = path.path.segments.last()?;
    if segment.ident != "Arc" { return None; }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else { return None; };
    match args.args.first()? {
        syn::GenericArgument::Type(inner) if args.args.len() == 1 => Some(inner),
        _ => None,
    }
}


fn derive_port_layout(input: DeriveInput) -> syn::Result<Tokens> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&input.ident,
            "PortLayout derive supports non-generic structs; implement PortLayout manually for generic types"));
    }
    let ident = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(ident, "PortLayout derive requires a struct"));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(ident,
            "PortLayout derive currently requires named fields; implement PortLayout manually for tuple/unit structs"));
    };

    let mut append = Vec::new();
    let mut assemble = Vec::new();
    let mut bounded_impls: Vec<Vec<(Tokens, Tokens)>> = (0..8).map(|_| Vec::new()).collect();
    for field in &fields.named {
        let name = field.ident.as_ref().expect("named field");
        let ty = &field.ty;
        let mut directive: Option<bool> = None; // true = flatten, false = atomic
        for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("port")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("flatten") {
                    if directive.replace(true).is_some() {
                        return Err(meta.error("use only one of port(flatten) or port(atomic)"));
                    }
                    Ok(())
                } else if meta.path.is_ident("atomic") {
                    if directive.replace(false).is_some() {
                        return Err(meta.error("use only one of port(flatten) or port(atomic)"));
                    }
                    Ok(())
                } else {
                    Err(meta.error("supported PortLayout field directives: flatten, atomic"))
                }
            })?;
        }
        let flatten = directive.unwrap_or_else(|| !scalar_field(ty));
        if flatten && ty.to_token_stream().to_string().split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|part| part == ident.to_string())
        {
            return Err(syn::Error::new_spanned(ty,
                "recursive PortLayout expansion is unsupported; keep the recursive field atomic or implement a bounded layout manually"));
        }
        let path = quote! {
            if prefix.is_empty() { ::std::string::ToString::to_string(stringify!(#name)) }
            else { ::std::format!("{prefix}.{}", stringify!(#name)) }
        };
        if flatten && tuple_field(ty) {
            append.push(quote! {
                let field_path = #path;
                <#ty as ::phenix_graph::PortLayoutAt<1>>::append_slots_at(&field_path, slots)?;
            });
            assemble.push(quote! {
                #name: {
                    let field_path = #path;
                    <#ty as ::phenix_graph::PortLayoutAt<1>>::from_selected_at(values, &field_path)?
                }
            });
        } else if flatten {
            append.push(quote! {
                let field_path = #path;
                <#ty as ::phenix_graph::PortLayoutAt<8>>::append_slots_at(&field_path, slots)?;
            });
            assemble.push(quote! {
                #name: {
                    let field_path = #path;
                    <#ty as ::phenix_graph::PortLayoutAt<8>>::from_selected_at(values, &field_path)?
                }
            });
        } else {
            append.push(quote! {
                let field_path = #path;
                ::phenix_graph::push_layout_slot(slots, field_path, ::phenix_graph::InputSlot::deferred::<#ty>)?;
            });
            assemble.push(quote! {
                #name: {
                    let field_path = #path;
                    values.get_path::<#ty>(&field_path)?
                }
            });
        }

        for depth in 1usize..=8 {
            let previous = depth - 1;
            let bounded_append = if flatten && tuple_field(ty) {
                quote! {
                    let field_path = #path;
                    <#ty as ::phenix_graph::PortLayoutAt<1>>::append_slots_at(&field_path, slots)?;
                }
            } else if flatten && directive == Some(true) {
                quote! {
                    let field_path = #path;
                    <#ty as ::phenix_graph::PortLayoutAt<8>>::append_slots_at(&field_path, slots)?;
                }
            } else if flatten {
                quote! {
                    let field_path = #path;
                    <#ty as ::phenix_graph::PortLayoutAt<#previous>>::append_slots_at(&field_path, slots)?;
                }
            } else {
                quote! {
                    let field_path = #path;
                    ::phenix_graph::push_layout_slot(slots, field_path, ::phenix_graph::InputSlot::deferred::<#ty>)?;
                }
            };
            let bounded_assemble = if flatten && tuple_field(ty) {
                quote! {
                    #name: {
                        let field_path = #path;
                        <#ty as ::phenix_graph::PortLayoutAt<1>>::from_selected_at(values, &field_path)?
                    }
                }
            } else if flatten && directive == Some(true) {
                quote! {
                    #name: {
                        let field_path = #path;
                        <#ty as ::phenix_graph::PortLayoutAt<8>>::from_selected_at(values, &field_path)?
                    }
                }
            } else if flatten {
                quote! {
                    #name: {
                        let field_path = #path;
                        <#ty as ::phenix_graph::PortLayoutAt<#previous>>::from_selected_at(values, &field_path)?
                    }
                }
            } else {
                quote! {
                    #name: {
                        let field_path = #path;
                        values.get_path::<#ty>(&field_path)?
                    }
                }
            };
            bounded_impls[depth - 1].push((bounded_append, bounded_assemble));
        }
    }
    let bounded_impls = bounded_impls.into_iter().enumerate().map(|(index, body)| {
        let depth = index + 1;
        let appends = body.iter().map(|(append, _)| append);
        let assemblies = body.iter().map(|(_, assemble)| assemble);
        quote! {
            impl ::phenix_graph::PortLayoutAt<#depth> for #ident {
                fn append_slots_at(prefix: &str, slots: &mut ::std::vec::Vec<::phenix_graph::InputSlot>)
                    -> ::std::result::Result<(), ::phenix_graph::LayoutError> {
                    #(#appends)*
                    Ok(())
                }
                fn from_selected_at(values: &::phenix_graph::SelectedValues<'_>, prefix: &str)
                    -> ::std::result::Result<Self, ::std::string::String>
                {
                    Ok(Self { #(#assemblies),* })
                }
            }
        }
    });
    Ok(quote! {
        impl ::phenix_graph::PortLayout for #ident {
            fn append_slots(prefix: &str, slots: &mut ::std::vec::Vec<::phenix_graph::InputSlot>)
                    -> ::std::result::Result<(), ::phenix_graph::LayoutError> {
                #(#append)*
                Ok(())
            }
            fn from_selected(values: &::phenix_graph::SelectedValues<'_>, prefix: &str)
                -> ::std::result::Result<Self, ::std::string::String>
            {
                Ok(Self { #(#assemble),* })
            }
        }
        #(#bounded_impls)*
    })
}

/// Derive a length-delimited canonical field codec for a named struct.
/// Contract schemas are explicit, NOT inferred from Rust type names or layout.
/// This is a proof-of-concept byte codec; not a general ABI for trait objects.
fn derive_contract_value(input: DeriveInput) -> syn::Result<Tokens> {
    let ident = &input.ident;
    let fields = fields(&input, false)?;
    let mut schema: Option<LitStr> = None;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("contract")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("schema") {
                schema = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("supported: schema = \"stable.schema.id\""))
            }
        })?;
    }
    let schema = schema.ok_or_else(|| syn::Error::new_spanned(ident,
        "#[derive(ContractValue)] requires #[contract(schema = \"stable.schema.id\")]"))?;
    if schema.value().trim().is_empty() {
        return Err(syn::Error::new_spanned(&schema, "schema must not be empty"));
    }
    // For each field, encode a length-prefixed payload. Optional fields get
    // a presence tag (0 = None, 1 = Some), independent of the inner codec.
    let write_fields = fields.iter().map(|f| {
        let ident = &f.ident;
        let data = if f.optional {
            quote! {
                match &self.#ident {
                    None => field.push(0),
                    Some(value) => {
                        field.push(1);
                        field.extend_from_slice(&::phenix_graph::ContractValue::encode(value));
                    }
                }
            }
        } else {
            quote! { field.extend_from_slice(&::phenix_graph::ContractValue::encode(&self.#ident)); }
        };
        quote! {
            let mut field = ::std::vec::Vec::new();
            #data
            out.extend_from_slice(&u64::try_from(field.len()).expect("field too large").to_le_bytes());
            out.extend_from_slice(&field);
        }
    });
    let read_fields = fields.iter().map(|f| {
        let ident = &f.ident;
        let ty = &f.value_type;
        let decoder = if f.optional {
            quote! {
                match field.split_first() {
                    Some((&0, rest)) if rest.is_empty() => None,
                    Some((&1, rest)) => Some(<#ty as ::phenix_graph::ContractValue>::decode(rest)?),
                    _ => return Err(format!("invalid Option encoding for {}", stringify!(#ident))),
                }
            }
        } else {
            quote! { <#ty as ::phenix_graph::ContractValue>::decode(field)? }
        };
        quote! {
            let header_end = cursor.checked_add(8).ok_or("field offset overflow")?;
            let header = bytes.get(cursor..header_end).ok_or("truncated field header")?;
            let length = usize::try_from(u64::from_le_bytes(
                header.try_into().map_err(|_| "invalid field header")?
            )).map_err(|_| "field length overflow")?;
            let end = header_end.checked_add(length).ok_or("field length overflow")?;
            let field = bytes.get(header_end..end).ok_or("truncated field payload")?;
            let #ident = #decoder;
            cursor = end;
        }
    });
    let identifiers = fields.iter().map(|f| &f.ident);
    Ok(quote! {
        impl ::phenix_graph::ContractValue for #ident {
            const SCHEMA: &'static str = #schema;
            fn encode(&self) -> ::std::vec::Vec<u8> {
                let mut out = ::std::vec::Vec::new();
                #(#write_fields)*
                out
            }
            fn decode(bytes: &[u8]) -> ::std::result::Result<Self, ::std::string::String> {
                let mut cursor = 0usize;
                #(#read_fields)*
                if cursor != bytes.len() { return Err("trailing bytes in contract value".into()); }
                Ok(Self { #(#identifiers),* })
            }
        }
    })
}

#[proc_macro_derive(PortArgs, attributes(slot))]
pub fn port_args(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_port_args(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

#[proc_macro_derive(PortLayout, attributes(port))]
pub fn port_layout(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_port_layout(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

#[proc_macro_derive(ContractValue, attributes(contract, slot))]
pub fn contract_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_contract_value(input).unwrap_or_else(syn::Error::into_compile_error).into()
}


/// In-process struct layout. No ContractValue implementation or byte encoding
/// is generated or required. Owned assembly clones selected leaf values; a
/// field of type Arc<T> can be used when T itself is not Clone.
fn derive_local_port_layout(input: DeriveInput) -> syn::Result<Tokens> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&input.ident,
            "LocalPortLayout derive currently supports non-generic named structs; implement the trait manually for generic types"));
    }
    let ident = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(ident, "LocalPortLayout requires a struct"));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(ident, "LocalPortLayout requires named fields"));
    };
    let mut specs = Vec::new();
    for field in &fields.named {
        let field_name = field.ident.as_ref().expect("named field").clone();
        let ty = field.ty.clone();
        let mut directive = None;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("port")) {
            attr.parse_nested_meta(|meta| {
                let value = if meta.path.is_ident("flatten") { true }
                    else if meta.path.is_ident("atomic") { false }
                    else { return Err(meta.error("supported: port(flatten), port(atomic)")); };
                if directive.replace(value).is_some() {
                    return Err(meta.error("duplicate or conflicting port layout directive"));
                }
                Ok(())
            })?;
        }
        let flatten = directive.unwrap_or_else(|| !scalar_field(&ty));
        if flatten && ty.to_token_stream().to_string()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|part| part == ident.to_string()) {
            return Err(syn::Error::new_spanned(&ty,
                "recursive local flattening requires an atomic field or an explicit manual bounded layout"));
        }
        specs.push((field_name, ty, flatten, directive == Some(true)));
    }
    let implementations = (1usize..=8).map(|depth| {
        let mut append = Vec::new();
        let mut assemble = Vec::new();
        for (field, ty, flatten, forced) in &specs {
            let path = quote! {
                if prefix.is_empty() { stringify!(#field).to_owned() }
                else { ::std::format!("{prefix}.{}", stringify!(#field)) }
            };
            let leaf_take = if let Some(inner) = arc_inner(ty) {
                quote! { values.take_shared_local_path::<#inner>(&path)? }
            } else {
                quote! { values.take_local_path::<#ty>(&path)? }
            };
            if *forced {
                // An explicit flatten override is not silently limited by
                // the surrounding depth budget.
                append.push(quote! {
                    let path = #path;
                    <#ty as ::phenix_graph::LocalPortLayout>::append_local_slots(&path, slots)?;
                });
                assemble.push(quote! {
                    #field: {
                        let path = #path;
                        <#ty as ::phenix_graph::LocalPortLayout>::from_local_owned(values, &path)?
                    }
                });
            } else if *flatten && depth > 1 {
                let remaining = depth - 1;
                append.push(quote! {
                    let path = #path;
                    <#ty as ::phenix_graph::LocalPortLayoutAt<#remaining>>::append_local_slots_at(&path, slots)?;
                });
                assemble.push(quote! {
                    #field: {
                        let path = #path;
                        <#ty as ::phenix_graph::LocalPortLayoutAt<#remaining>>::from_local_owned_at(values, &path)?
                    }
                });
            } else {
                append.push(quote! {
                    let path = #path;
                    ::phenix_graph::push_layout_slot(slots, path, ::phenix_graph::InputSlot::local::<#ty>)?;
                });
                assemble.push(quote! {
                    #field: {
                        let path = #path;
                        #leaf_take
                    }
                });
            }
        }
        quote! {
            impl ::phenix_graph::LocalPortLayoutAt<#depth> for #ident {
                fn append_local_slots_at(prefix: &str, slots: &mut ::std::vec::Vec<::phenix_graph::InputSlot>)
                    -> ::std::result::Result<(), ::phenix_graph::LayoutError> {
                    #(#append)*
                    Ok(())
                }
                fn from_local_owned_at(values: &mut ::phenix_graph::OwnedSelectedValues, prefix: &str)
                    -> ::std::result::Result<Self, ::std::string::String> {
                    Ok(Self { #(#assemble),* })
                }
            }
        }
    });
    // Deep is structurally recursive, not an alias for depth eight.
    // Nested user types must implement LocalPortLayout; atomic fields remain
    // leaves and do not need to expose a layout of their own.
    let mut deep_append = Vec::new();
    let mut deep_assemble = Vec::new();
    for (field, ty, flatten, _) in &specs {
        let path = quote! {
            if prefix.is_empty() { stringify!(#field).to_owned() }
            else { ::std::format!("{prefix}.{}", stringify!(#field)) }
        };
        let leaf_take = if let Some(inner) = arc_inner(ty) {
            quote! { values.take_shared_local_path::<#inner>(&path)? }
        } else {
            quote! { values.take_local_path::<#ty>(&path)? }
        };
        if *flatten {
            deep_append.push(quote! {
                let path = #path;
                <#ty as ::phenix_graph::LocalPortLayout>::append_local_slots(&path, slots)?;
            });
            deep_assemble.push(quote! {
                #field: {
                    let path = #path;
                    <#ty as ::phenix_graph::LocalPortLayout>::from_local_owned(values, &path)?
                }
            });
        } else {
            deep_append.push(quote! {
                let path = #path;
                ::phenix_graph::push_layout_slot(slots, path, ::phenix_graph::InputSlot::local::<#ty>)?;
            });
            deep_assemble.push(quote! {
                #field: {
                    let path = #path;
                    #leaf_take
                }
            });
        }
    }
    let keys = specs.iter().map(|(field,ty,_,_)| {
        let upper = syn::Ident::new(&format!("SLOT_{}",field.to_string().to_uppercase()), field.span());
        quote! {
            pub const #upper: ::phenix_graph::authoring::SlotField<Self, #ty> =
                ::phenix_graph::authoring::SlotField::new(stringify!(#field));
        }
    });
    Ok(quote! {
        #(#implementations)*
        impl ::phenix_graph::LocalPortLayout for #ident {
            fn append_local_slots(prefix: &str, slots: &mut ::std::vec::Vec<::phenix_graph::InputSlot>)
                    -> ::std::result::Result<(), ::phenix_graph::LayoutError> {
                #(#deep_append)*
                Ok(())
            }
            fn from_local_owned(values: &mut ::phenix_graph::OwnedSelectedValues, prefix: &str)
                -> ::std::result::Result<Self, ::std::string::String> {
                Ok(Self { #(#deep_assemble),* })
            }
        }
        impl #ident { #(#keys)* }
    })
}

#[proc_macro_derive(LocalPortLayout, attributes(port))]
pub fn local_port_layout(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_local_port_layout(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

// Optional patch syntax. This lowers exclusively to the public PatchBuilder
// methods: no compiler/executor mechanism exists behind the proc macro.
mod patch_syntax {
    use proc_macro2::{Span, TokenStream as Tokens};
    use quote::quote;
    use syn::{parse::{Parse, ParseStream}, Expr, Ident, LitInt, LitStr, Result, Token};

    enum Statement {
        Node(Ident, Expr, Option<Expr>),
        Instance(Ident, Expr),
        Replace(Ident, Expr),
        Connect(Vec<String>),
        Disconnect(String, String),
        Input(String, String),
        Output(String, String),
        Trigger(String, Vec<String>),
        After(Vec<String>),
        Priority(LitInt),
    }
    pub struct PatchSyntax { id: LitStr, statements: Vec<Statement> }

    fn name(input: ParseStream<'_>) -> Result<String> {
        if input.peek(LitStr) { Ok(input.parse::<LitStr>()?.value()) }
        else { Ok(input.parse::<Ident>()?.to_string()) }
    }
    fn endpoint(input: ParseStream<'_>) -> Result<String> {
        let mut output = input.parse::<Ident>()?.to_string();
        while input.peek(Token![.]) || input.peek(Token![/]) {
            if input.peek(Token![.]) { input.parse::<Token![.]>()?; output.push('.'); }
            else { input.parse::<Token![/]>()?; output.push('/'); }
            output.push_str(&input.parse::<Ident>()?.to_string());
        }
        Ok(output)
    }
    fn names(input: ParseStream<'_>) -> Result<Vec<String>> {
        let content;
        syn::bracketed!(content in input);
        let mut names = Vec::new();
        while !content.is_empty() {
            names.push(name(&content)?);
            if content.is_empty() { break; }
            content.parse::<Token![,]>()?;
        }
        Ok(names)
    }
    impl Parse for PatchSyntax {
        fn parse(input: ParseStream<'_>) -> Result<Self> {
            let mut id = None;
            let mut statements = Vec::new();
            while !input.is_empty() {
                let first: Ident = input.parse()?;
                match first.to_string().as_str() {
                    "id" => {
                        input.parse::<Token![:]>()?;
                        if id.is_some() { return Err(syn::Error::new(first.span(), "duplicate patch id")); }
                        id = Some(input.parse::<LitStr>()?);
                    }
                    "after" | "requires" => {
                        input.parse::<Token![:]>()?;
                        statements.push(Statement::After(names(input)?));
                    }
                    "priority" => {
                        input.parse::<Token![:]>()?;
                        statements.push(Statement::Priority(input.parse()?));
                    }
                    "node" => {
                        let n: Ident = input.parse()?;
                        input.parse::<Token![=]>()?;
                        let handler: Expr = input.parse()?;
                        let layout = if input.peek(Token![,]) {
                            input.parse::<Token![,]>()?;
                            let label: Ident = input.parse()?;
                            if label != "layout" { return Err(syn::Error::new(label.span(), "expected `layout = ...`")); }
                            input.parse::<Token![=]>()?;
                            Some(input.parse::<Expr>()?)
                        } else { None };
                        statements.push(Statement::Node(n, handler, layout));
                    }
                    "instance" => {
                        let n: Ident = input.parse()?;
                        input.parse::<Token![=]>()?;
                        statements.push(Statement::Instance(n, input.parse()?));
                    }
                    "replace" => {
                        let n: Ident = input.parse()?;
                        input.parse::<Token![=]>()?;
                        statements.push(Statement::Replace(n, input.parse()?));
                    }
                    "input" | "output" => {
                        let exported = name(input)?;
                        input.parse::<Token![=]>()?;
                        let path = endpoint(input)?;
                        statements.push(if first == "input" {
                            Statement::Input(exported, path)
                        } else { Statement::Output(exported, path) });
                    }
                    "trigger" => {
                        let exported = name(input)?;
                        input.parse::<Token![=]>()?;
                        statements.push(Statement::Trigger(exported, names(input)?));
                    }
                    "disconnect" => {
                        let from = endpoint(input)?;
                        input.parse::<Token![->]>()?;
                        let to = endpoint(input)?;
                        statements.push(Statement::Disconnect(from, to));
                    }
                    "connect" => {
                        let mut chain = vec![endpoint(input)?];
                        while input.peek(Token![->]) {
                            input.parse::<Token![->]>()?;
                            chain.push(endpoint(input)?);
                        }
                        if chain.len() < 2 { return Err(syn::Error::new(first.span(), "a connection needs two endpoints")); }
                        statements.push(Statement::Connect(chain));
                    }
                    _ => {
                        let mut chain = vec![first.to_string()];
                        while input.peek(Token![.]) || input.peek(Token![/]) {
                            if input.peek(Token![.]) { input.parse::<Token![.]>()?; chain[0].push('.'); }
                            else { input.parse::<Token![/]>()?; chain[0].push('/'); }
                            chain[0].push_str(&input.parse::<Ident>()?.to_string());
                        }
                        while input.peek(Token![->]) {
                            input.parse::<Token![->]>()?;
                            chain.push(endpoint(input)?);
                        }
                        if chain.len() < 2 { return Err(syn::Error::new(first.span(), "unknown patch statement")); }
                        statements.push(Statement::Connect(chain));
                    }
                }
                input.parse::<Token![;]>()?;
            }
            let id = id.ok_or_else(|| syn::Error::new(Span::call_site(), "patch requires `id: \"...\";`"))?;
            Ok(Self { id, statements })
        }
    }
    impl PatchSyntax {
        pub fn expand(self) -> Tokens {
            let id = self.id;
            let mut actions = Vec::new();
            for statement in self.statements {
                let action = match statement {
                    Statement::Node(n, handler, None) => {
                        let name = n.to_string();
                        quote! { __patch.node(#name, #handler); }
                    }
                    Statement::Node(n, handler, Some(layout)) => {
                        let name = n.to_string();
                        quote! { __patch.node(#name, ::phenix_graph::Node::with_layout(#handler, #layout)); }
                    }
                    Statement::Instance(n, expression) => {
                        let name = n.to_string();
                        quote! { __patch.instance(#name, #expression); }
                    }
                    Statement::Replace(n, handler) => {
                        let name = n.to_string();
                        // The macro is optional sugar over the same validated
                        // NodeTemplate replacement used by macro-free Rust.
                        // A closure is inferred as a local typed action; an
                        // explicit template/module expression passes through.
                        let replacement = if matches!(&handler, syn::Expr::Closure(_)) {
                            quote! { ::phenix_graph::NodeTemplate::local_function(#handler) }
                        } else {
                            quote! { #handler }
                        };
                        quote! { __patch.replace(#name, #replacement); }
                    }
                    Statement::Connect(chain) => {
                        let pairs = chain.windows(2).map(|p| {
                            let from = &p[0]; let to = &p[1];
                            quote! { __patch.connect(#from, #to)?; }
                        });
                        quote! { #(#pairs)* }
                    }
                    Statement::Disconnect(from, to) => quote! { __patch.disconnect(#from, #to)?; },
                    Statement::Input(name, path) => quote! { __patch.expose_input(#name, #path); },
                    Statement::Output(name, path) => quote! { __patch.expose_output(#name, #path); },
                    Statement::Trigger(name, values) => quote! { __patch.trigger(#name, &[#(#values),*]); },
                    Statement::After(values) => {
                        let deps = values.iter().map(|dep| quote! { __patch.after(#dep); });
                        quote! { #(#deps)* }
                    }
                    Statement::Priority(priority) => quote! { __patch.priority(#priority); },
                };
                actions.push(action);
            }
            quote! {{
                (|| -> ::core::result::Result<::phenix_graph::PatchBuilder, ::phenix_graph::PatchError> {
                    let mut __patch = ::phenix_graph::PatchBuilder::new(#id);
                    #(#actions)*
                    ::core::result::Result::Ok(__patch)
                })()
            }}
        }
    }
}

#[proc_macro]
pub fn patch(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as patch_syntax::PatchSyntax);
    input.expand().into()
}
