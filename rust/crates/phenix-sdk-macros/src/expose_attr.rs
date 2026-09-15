use crate::component_runtime_attr;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeSet;
use syn::{
    parse::Parser, parse_quote, punctuated::Punctuated, spanned::Spanned, Attribute, Expr, ExprLit,
    Fields, ImplItem, ItemImpl, ItemStruct, Lit, LitStr, Meta, Token, Type,
};

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "exposed Rust types do not accept root arguments",
        ));
    }
    if let Ok(item) = syn::parse2::<ItemStruct>(input.clone()) {
        return expand_struct(item);
    }
    if let Ok(item) = syn::parse2::<ItemImpl>(input.clone()) {
        return expand_impl(item);
    }
    Err(syn::Error::new_spanned(
        input,
        "#[phenix_sdk::expose] applies to a struct or inherent impl",
    ))
}

fn expand_struct(mut item: ItemStruct) -> syn::Result<TokenStream> {
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "exposed Rust types must have one concrete projection",
        ));
    }

    let fields = exposed_fields(&mut item)?;
    let name = &item.ident;
    let fields_impl = exposed_fields_impl(name, &fields);

    Ok(quote! {
        #item
        #fields_impl
    })
}

pub(crate) fn exposed_fields_impl(name: &syn::Ident, fields: &[ExposedField]) -> TokenStream {
    let export_fields = fields.iter().map(|field| {
        let ty = &field.ty;
        let mount = &field.mount;
        quote! {
            exports.extend(
                <#ty as ::phenix_sdk::StaticExpose>::exposed_exports()
                    .into_iter()
                    .map(|export| ::phenix_sdk::remount_exposed_export(owner, #mount, export)),
            );
        }
    });
    let dispatch_fields = fields.iter().map(|field| {
        let member = &field.field;
        let ty = &field.ty;
        let mount = &field.mount;
        quote! {
            if let Some(service) =
                ::phenix_sdk::remap_exposed_service::<#ty>(owner, #mount, service)
            {
                return Some(<#ty as ::phenix_sdk::StaticExpose>::dispatch_exposed(
                    &self.#member,
                    &service,
                    input,
                    host,
                ));
            }
        }
    });

    quote! {
        impl ::phenix_sdk::StaticExposeFields for #name {
            fn exposed_field_exports_for(
                owner: &str,
            ) -> Vec<::phenix_sdk::StaticComponentExport> {
                let mut exports = Vec::new();
                #(#export_fields)*
                exports
            }

            fn dispatch_exposed_field_for(
                &self,
                owner: &str,
                service: &::phenix_sdk::__phenix_plugin::ServiceId,
                input: &[u8],
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Option<Result<Vec<u8>, String>> {
                #(#dispatch_fields)*
                None
            }
        }
    }
}

fn expand_impl(mut item: ItemImpl) -> syn::Result<TokenStream> {
    if item.trait_.is_some() || !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item,
            "exposed behavior must be a non-generic inherent impl",
        ));
    }
    let self_ty = (*item.self_ty).clone();
    let self_name = type_ident(&self_ty)?;
    let mut markers = Vec::new();
    let mut names = BTreeSet::new();

    for member in &mut item.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        let mut retained = Vec::new();
        let mut contribution = None;
        for attribute in std::mem::take(&mut method.attrs) {
            if !attribute.path().is_ident("phenix") {
                retained.push(attribute);
                continue;
            }
            if contribution.is_some() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "an exposed method may declare only one Phenix role",
                ));
            }
            let Some(expose) = expose_declaration(&attribute, &method.sig.ident.to_string())?
            else {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "nested exposed behavior currently accepts only #[phenix(expose...)] methods",
                ));
            };
            contribution = Some(expose);
        }

        if let Some(expose) = contribution {
            let public_name = expose.name;
            if !names.insert(public_name.value()) {
                return Err(syn::Error::new_spanned(
                    public_name,
                    "duplicate exposed method path",
                ));
            }
            let marker = format_ident!("__PhenixExposed{}_{}", self_name, method.sig.ident);
            let authority = expose.authority;
            retained.push(match authority {
                Some(authority) => parse_quote! {
                    #[phenix(export(#marker), public, authority = #authority)]
                },
                None => parse_quote! {
                    #[phenix(export(#marker), public)]
                },
            });
            markers.push(quote! {
                #[doc(hidden)]
                #[allow(non_camel_case_types)]
                struct #marker;

                impl ::phenix_sdk::InterfaceMarker for #marker {
                    fn interface_id() -> ::phenix_sdk::__phenix_plugin::InterfaceId {
                        ::phenix_sdk::exposed_interface::<#self_ty>(#public_name)
                    }
                }
            });
        }
        method.attrs = retained;
    }

    let expanded = component_runtime_attr::expand(TokenStream::new(), quote!(#item))?;
    Ok(quote! {
        #expanded
        #(#markers)*
    })
}

pub(crate) struct ExposedField {
    field: syn::Ident,
    ty: Type,
    mount: LitStr,
}

pub(crate) fn exposed_fields(item: &mut ItemStruct) -> syn::Result<Vec<ExposedField>> {
    take_exposed_fields(item, false)
}

pub(crate) fn plugin_exposed_fields(item: &mut ItemStruct) -> syn::Result<Vec<ExposedField>> {
    take_exposed_fields(item, true)
}

fn take_exposed_fields(
    item: &mut ItemStruct,
    allow_other_roles: bool,
) -> syn::Result<Vec<ExposedField>> {
    let Fields::Named(fields) = &mut item.fields else {
        return Ok(Vec::new());
    };
    let mut exposed = Vec::new();
    let mut mounts = BTreeSet::new();

    for field in &mut fields.named {
        let mut retained = Vec::new();
        let mut contribution = None;
        let phenix_roles = field
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("phenix"))
            .count();
        for attribute in std::mem::take(&mut field.attrs) {
            if !attribute.path().is_ident("phenix") {
                retained.push(attribute);
                continue;
            }
            let expose = expose_declaration(
                &attribute,
                field
                    .ident
                    .as_ref()
                    .expect("named field has an identifier")
                    .to_string()
                    .as_str(),
            )?;
            let expose = match expose {
                Some(expose) => expose,
                None if allow_other_roles => {
                    retained.push(attribute);
                    continue;
                }
                None => {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "exposed struct fields accept only #[phenix(expose...)]",
                    ));
                }
            };
            if phenix_roles > 1 || contribution.is_some() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "an exposed field may declare only one Phenix role",
                ));
            }
            if expose.authority.is_some() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "exposed fields do not accept authority metadata",
                ));
            }
            contribution = Some(expose);
        }
        if let Some(expose) = contribution {
            let mount = expose.name;
            if !mounts.insert(mount.value()) {
                return Err(syn::Error::new_spanned(
                    mount,
                    "duplicate exposed field path",
                ));
            }
            exposed.push(ExposedField {
                field: field.ident.clone().expect("named field has an identifier"),
                ty: field.ty.clone(),
                mount,
            });
        }
        field.attrs = retained;
    }
    Ok(exposed)
}

struct ExposeDeclaration {
    name: LitStr,
    authority: Option<Expr>,
}

fn expose_declaration(
    attribute: &Attribute,
    default_name: &str,
) -> syn::Result<Option<ExposeDeclaration>> {
    let Meta::List(meta) = &attribute.meta else {
        return Ok(None);
    };
    let outer = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let Some(first) = outer.first() else {
        return Ok(None);
    };

    let modifiers = match first {
        Meta::Path(path) if path.is_ident("expose") => {
            outer.iter().skip(1).cloned().collect::<Vec<_>>()
        }
        Meta::List(list) if list.path.is_ident("expose") => {
            if outer.len() != 1 {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "expose metadata belongs inside expose(...) when that form is used",
                ));
            }
            Punctuated::<Meta, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())?
                .into_iter()
                .collect::<Vec<_>>()
        }
        _ => return Ok(None),
    };

    let mut name = None;
    let mut authority = None;
    for modifier in modifiers {
        let Meta::NameValue(value) = modifier else {
            return Err(syn::Error::new_spanned(
                modifier,
                "expose metadata must use name = \"...\" or authority = <expression>",
            ));
        };
        if value.path.is_ident("name") {
            if name.is_some() {
                return Err(syn::Error::new_spanned(value, "duplicate expose path"));
            }
            let path = match value.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) => value,
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "expose path must be a string literal",
                    ));
                }
            };
            validate_public_segment(&path)?;
            name = Some(path);
        } else if value.path.is_ident("authority") {
            if authority.is_some() {
                return Err(syn::Error::new_spanned(value, "duplicate expose authority"));
            }
            authority = Some(value.value);
        } else {
            return Err(syn::Error::new_spanned(
                value.path,
                "unsupported expose metadata",
            ));
        }
    }

    let name = name.unwrap_or_else(|| LitStr::new(default_name, attribute.span()));
    validate_public_segment(&name)?;
    Ok(Some(ExposeDeclaration { name, authority }))
}

fn validate_public_segment(value: &LitStr) -> syn::Result<()> {
    let name = value.value();
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(syn::Error::new_spanned(
            value,
            "public member name must be one non-empty alphanumeric, '_' or '-' path segment",
        ));
    }
    Ok(())
}

fn type_ident(ty: &Type) -> syn::Result<syn::Ident> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "exposed behavior requires a concrete path self type",
        ));
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.clone())
        .ok_or_else(|| syn::Error::new_spanned(ty, "exposed self type is empty"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn exposed_struct_recurses_only_through_marked_fields() {
        let output = expand(
            TokenStream::new(),
            quote! {
                struct Models {
                    #[phenix(expose(name = "providers"))]
                    registry: Providers,
                    cache: Cache,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticExposeFields for Models"));
        assert!(output.contains("StaticExpose > :: exposed_exports"));
        assert!(output.contains("providers"));
        assert!(!output.contains("phenix (expose"));
    }

    #[test]
    fn exposed_impl_lowers_only_marked_methods() {
        let output = expand(
            TokenStream::new(),
            quote! {
                impl Models {
                    #[phenix(expose(name = "current"))]
                    fn selected(&self, request: Request) -> Response {
                        todo!()
                    }

                    fn refresh_cache(&mut self) {}
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticComponentBehavior for Models"));
        assert!(output.contains("exposed_interface :: < Models >"));
        assert!(output.contains("current"));
        assert!(output.contains("refresh_cache"));
        assert!(!output.contains("phenix (expose"));
    }

    #[test]
    fn exposed_members_reject_multi_segment_local_names() {
        let error = expand(
            TokenStream::new(),
            quote! {
                struct Models {
                    #[phenix(expose(name = "nested/providers"))]
                    registry: Providers,
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("one non-empty"));
    }

    #[test]
    fn exposed_fields_reject_authority_and_multiple_roles() {
        let authority = expand(
            TokenStream::new(),
            quote! {
                struct Models {
                    #[phenix(expose(authority = Authority::default()))]
                    registry: Providers,
                }
            },
        )
        .unwrap_err();
        assert!(authority.to_string().contains("do not accept authority"));

        let roles = expand(
            TokenStream::new(),
            quote! {
                struct Models {
                    #[phenix(expose)]
                    #[phenix(expose(name = "providers"))]
                    registry: Providers,
                }
            },
        )
        .unwrap_err();
        assert!(roles.to_string().contains("only one Phenix role"));
    }

    #[test]
    fn exposed_members_reject_removed_path_alias() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Models {
                    #[phenix(expose(path = "current"))]
                    fn selected(&mut self) {}
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("unsupported expose metadata"));
    }
}
