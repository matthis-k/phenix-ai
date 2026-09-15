use crate::{component_attr, interface_attr::validate_interface_id};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse::Parser, parse_quote, punctuated::Punctuated, Attribute, FnArg, GenericArgument,
    ImplItem, ItemImpl, LitStr, Meta, PathArguments, ReturnType, Token, Type,
};

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let Ok(mut item) = syn::parse2::<ItemImpl>(input.clone()) else {
        return component_attr::expand(args, input);
    };
    normalize_layer_priorities(&mut item)?;
    let runtime = runtime_impl(&item)?;
    let expanded = component_attr::expand(args, quote!(#item))?;
    Ok(quote! {
        #expanded
        #runtime
    })
}

fn normalize_layer_priorities(item: &mut ItemImpl) -> syn::Result<()> {
    for member in &mut item.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        for attribute in &mut method.attrs {
            let Some(mut arguments) = layer_arguments(attribute)? else {
                continue;
            };
            if arguments.iter().any(|argument| {
                matches!(argument, Meta::NameValue(value) if value.path.is_ident("priority"))
            }) {
                continue;
            }
            arguments.push(parse_quote!(priority = 0));
            attribute.meta = syn::parse2(quote!(phenix(layer(#arguments))))?;
        }
    }
    Ok(())
}

fn layer_arguments(attribute: &Attribute) -> syn::Result<Option<Punctuated<Meta, Token![,]>>> {
    if !attribute.path().is_ident("phenix") {
        return Ok(None);
    }
    let Meta::List(meta) = &attribute.meta else {
        return Ok(None);
    };
    let outer = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    if outer.len() != 1 {
        return Ok(None);
    }
    let Some(Meta::List(layer)) = outer.first() else {
        return Ok(None);
    };
    if !layer.path.is_ident("layer") {
        return Ok(None);
    }
    Ok(Some(
        Punctuated::<Meta, Token![,]>::parse_terminated.parse2(layer.tokens.clone())?,
    ))
}

fn runtime_impl(item: &ItemImpl) -> syn::Result<TokenStream> {
    if item.trait_.is_some() || !item.generics.params.is_empty() {
        return Ok(TokenStream::new());
    }
    let self_ty = &item.self_ty;
    let mut exports = Vec::new();
    let mut layers = Vec::new();
    let mut listeners = Vec::new();

    for member in &item.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        let Some(contribution) = contribution(&method.attrs)? else {
            continue;
        };
        match contribution {
            Contribution::Export(interface) => {
                validate_shared_receiver(method, "exported component")?;
                exports.push(export_arm(method, interface)?);
            }
            Contribution::Layer(interface) => {
                validate_shared_receiver(method, "component layer")?;
                layers.push(layer_arm(method, interface)?);
            }
            Contribution::Listener => {
                validate_shared_receiver(method, "component listener")?;
                listeners.push(listener_arm(method)?);
            }
            Contribution::Other => {}
        }
    }

    Ok(quote! {
        impl ::phenix_sdk::StaticComponentRuntimeDispatch for #self_ty {
            fn dispatch_runtime(
                &self,
                service: &::phenix_sdk::__phenix_plugin::ServiceId,
                input: &[u8],
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                #(#exports)*
                Err(format!("unsupported component service: {service}"))
            }

            fn dispatch_layer_runtime(
                &self,
                service: &::phenix_sdk::__phenix_plugin::ServiceId,
                input: &[u8],
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Option<Result<::phenix_sdk::LayerResult, String>> {
                #(#layers)*
                None
            }

            fn dispatch_listener_runtime(
                &self,
                listener: &str,
                context: &::phenix_sdk::EventContext,
                payload: &[u8],
            ) -> Option<Result<(), Box<dyn ::std::error::Error + Send + Sync>>> {
                #(#listeners)*
                None
            }
        }
    })
}

fn validate_shared_receiver(method: &syn::ImplItemFn, role: &str) -> syn::Result<()> {
    let Some(FnArg::Receiver(receiver)) = method.sig.inputs.first() else {
        return Ok(());
    };
    if receiver.mutability.is_none() {
        return Ok(());
    }
    Err(syn::Error::new_spanned(
        receiver,
        format!(
            "{role} methods must use &self; move mutable state behind an explicit synchronization handle"
        ),
    ))
}

enum Contribution {
    Export(Interface),
    Layer(Interface),
    Listener,
    Other,
}

enum Interface {
    Literal(LitStr),
    Marker(Box<Type>),
}

impl Interface {
    fn expression(&self) -> TokenStream {
        match self {
            Self::Literal(id) => quote! {
                ::phenix_sdk::__phenix_plugin::InterfaceId::parse(#id)
                    .expect("component attribute validated the static interface id")
            },
            Self::Marker(marker) => quote! {
                <#marker as ::phenix_sdk::InterfaceMarker>::interface_id()
            },
        }
    }
}

fn contribution(attributes: &[Attribute]) -> syn::Result<Option<Contribution>> {
    for attribute in attributes {
        if !attribute.path().is_ident("phenix") {
            continue;
        }
        let Meta::List(meta) = &attribute.meta else {
            continue;
        };
        let outer = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
        let Some(Meta::List(kind)) = outer.first() else {
            continue;
        };
        if kind.path.is_ident("export") {
            return Ok(Some(Contribution::Export(parse_interface(&kind.tokens)?)));
        }
        if kind.path.is_ident("layer") {
            let arguments =
                Punctuated::<Meta, Token![,]>::parse_terminated.parse2(kind.tokens.clone())?;
            let Some(first) = arguments.first() else {
                return Err(syn::Error::new_spanned(
                    kind,
                    "layer requires an interface marker type",
                ));
            };
            let Meta::Path(path) = first else {
                return Err(syn::Error::new_spanned(
                    first,
                    "layer requires an interface marker type",
                ));
            };
            let marker = Type::Path(syn::TypePath {
                qself: None,
                path: path.clone(),
            });
            return Ok(Some(Contribution::Layer(Interface::Marker(Box::new(
                marker,
            )))));
        }
        if kind.path.is_ident("listen") {
            return Ok(Some(Contribution::Listener));
        }
        return Ok(Some(Contribution::Other));
    }
    Ok(None)
}

fn parse_interface(tokens: &TokenStream) -> syn::Result<Interface> {
    if let Ok(id) = syn::parse2::<LitStr>(tokens.clone()) {
        validate_interface_id(&id.value()).map_err(|error| syn::Error::new_spanned(&id, error))?;
        return Ok(Interface::Literal(id));
    }
    Ok(Interface::Marker(Box::new(syn::parse2(tokens.clone())?)))
}

#[derive(Clone, Copy)]
enum Projection {
    Projected,
    Project,
    Exact,
}

#[derive(Clone, Copy)]
enum ExportContext {
    Call,
    Plugin,
}

fn unwrap_projection(ty: &Type) -> syn::Result<(Type, Projection)> {
    let Type::Path(path) = ty else {
        return Ok((ty.clone(), Projection::Projected));
    };
    let Some(segment) = path.path.segments.last() else {
        return Ok((ty.clone(), Projection::Projected));
    };
    let projection = if segment.ident == "Project" {
        Projection::Project
    } else if segment.ident == "Exact" {
        Projection::Exact
    } else {
        return Ok((ty.clone(), Projection::Projected));
    };
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "structural wrapper requires one payload type",
        ));
    };
    if arguments.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            arguments,
            "structural wrapper requires one payload type",
        ));
    }
    let Some(GenericArgument::Type(inner)) = arguments.args.first() else {
        return Err(syn::Error::new_spanned(
            arguments,
            "structural wrapper requires one payload type",
        ));
    };
    Ok((inner.clone(), projection))
}

fn export_arm(method: &syn::ImplItemFn, interface: Interface) -> syn::Result<TokenStream> {
    let name = &method.sig.ident;
    let mut inputs = method.sig.inputs.iter();
    let _receiver = inputs.next();
    let context = inputs.clone().next().and_then(export_context);
    if context.is_some() {
        inputs.next();
    }
    let request = match inputs.next() {
        Some(FnArg::Typed(request)) => Some((*request.ty).clone()),
        _ => None,
    };
    let (decoded, projection) = request
        .as_ref()
        .map(unwrap_projection)
        .transpose()?
        .unwrap_or((parse_quote!(()), Projection::Projected));
    let interface = interface.expression();
    let decode = match projection {
        Projection::Exact => {
            quote!(::phenix_sdk::decode_exact_runtime::<#decoded>(host, &interface, input)?)
        }
        Projection::Projected | Projection::Project => {
            quote!(::phenix_sdk::decode_projected_runtime::<#decoded>(host, &interface, input)?)
        }
    };
    let request_arg = match projection {
        Projection::Project => quote!(::phenix_sdk::Project(request)),
        Projection::Exact => quote!(::phenix_sdk::Exact(request)),
        Projection::Projected => quote!(request),
    };
    let context_arg = match context {
        Some(ExportContext::Call) => Some(quote!(&plugin_context.call)),
        Some(ExportContext::Plugin) => Some(quote!(&plugin_context)),
        None => None,
    };
    let call = match (context_arg, request.is_some()) {
        (Some(context), true) => quote!(self.#name(#context, #request_arg)),
        (Some(context), false) => quote!(self.#name(#context)),
        (None, true) => quote!(self.#name(#request_arg)),
        (None, false) => quote!(self.#name()),
    };
    let call = if method.sig.asyncness.is_some() {
        quote!(::phenix_sdk::block_on_static(#call))
    } else {
        call
    };
    let encode = if returns_result(&method.sig.output) {
        quote!(::phenix_sdk::encode_result_runtime)
    } else {
        quote!(::phenix_sdk::encode_runtime)
    };
    let request_binding = request.is_some().then(|| quote!(let request = #decode;));

    Ok(quote! {
        {
            let interface = #interface;
            if service.as_str() == interface.as_str() {
                let plugin_context = ::phenix_sdk::PluginContext::new(host, (), (), ());
                #request_binding
                let response = #call;
                return #encode(host, &response);
            }
        }
    })
}

fn layer_arm(method: &syn::ImplItemFn, interface: Interface) -> syn::Result<TokenStream> {
    let name = &method.sig.ident;
    let mut inputs = method.sig.inputs.iter();
    let _receiver = inputs.next();
    let has_context = inputs
        .clone()
        .next()
        .is_some_and(|argument| is_named_ref(argument, "LayerContext"));
    if has_context {
        inputs.next();
    }
    let request = match inputs.next() {
        Some(FnArg::Typed(request)) => Some((*request.ty).clone()),
        _ => None,
    };
    if inputs.next().is_some() {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "layers accept at most &LayerContext and one typed request",
        ));
    }
    let (decoded, projection) = request
        .as_ref()
        .map(unwrap_projection)
        .transpose()?
        .unwrap_or((parse_quote!(()), Projection::Projected));
    let interface = interface.expression();
    let decode = match projection {
        Projection::Exact => {
            quote!(::phenix_sdk::decode_exact_runtime::<#decoded>(host, &interface, input))
        }
        Projection::Projected | Projection::Project => {
            quote!(::phenix_sdk::decode_projected_runtime::<#decoded>(host, &interface, input))
        }
    };
    let request_arg = match projection {
        Projection::Project => quote!(::phenix_sdk::Project(request)),
        Projection::Exact => quote!(::phenix_sdk::Exact(request)),
        Projection::Projected => quote!(request),
    };
    let call = match (has_context, request.is_some()) {
        (true, true) => quote!(self.#name(&layer_context, #request_arg)),
        (true, false) => quote!(self.#name(&layer_context)),
        (false, true) => quote!(self.#name(#request_arg)),
        (false, false) => quote!(self.#name()),
    };
    let call = if method.sig.asyncness.is_some() {
        quote!(::phenix_sdk::block_on_static(#call))
    } else {
        call
    };
    let result = match (
        returns_result(&method.sig.output),
        success_type(&method.sig.output).is_some_and(|ty| type_ends_with(ty, "LayerResult")),
    ) {
        (true, true) => quote!((#call).map_err(|error| error.to_string())),
        (false, true) => quote!(Ok(#call)),
        (true, false) => quote!({
            (#call).map_err(|error| error.to_string())?;
            layer_context.continue_input(input)
        }),
        (false, false) => quote!({
            #call;
            layer_context.continue_input(input)
        }),
    };
    let request_binding = request.is_some().then(|| {
        quote! {
            let request = match #decode {
                Ok(request) => request,
                Err(error) => return Some(Err(error)),
            };
        }
    });

    Ok(quote! {
        {
            let interface = #interface;
            if service.as_str() == interface.as_str() {
                let layer_context = ::phenix_sdk::LayerContext::from_host(host);
                #request_binding
                return Some(#result);
            }
        }
    })
}

fn listener_arm(method: &syn::ImplItemFn) -> syn::Result<TokenStream> {
    let name = &method.sig.ident;
    let listener = LitStr::new(&name.to_string(), name.span());
    let mut inputs = method.sig.inputs.iter();
    let _receiver = inputs.next();
    let context = inputs.next();
    if !context.is_some_and(|argument| is_named_ref(argument, "EventContext")) {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "listeners require &EventContext followed by one typed event payload",
        ));
    }
    let payload = match inputs.next() {
        Some(FnArg::Typed(payload)) => (*payload.ty).clone(),
        _ => {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "listeners require &EventContext followed by one typed event payload",
            ));
        }
    };
    if inputs.next().is_some() {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "listeners require &EventContext followed by exactly one typed event payload",
        ));
    }
    let (decoded, projection) = unwrap_projection(&payload)?;
    let decode = match projection {
        Projection::Exact => quote! {
            <Self as ::phenix_sdk::StaticComponentRuntimeDispatch>::decode_exact_listener_runtime::<#decoded>(
                payload,
            )
        },
        Projection::Projected | Projection::Project => quote! {
            <Self as ::phenix_sdk::StaticComponentRuntimeDispatch>::decode_projected_listener_runtime::<#decoded>(
                payload,
            )
        },
    };
    let payload_arg = match projection {
        Projection::Project => quote!(::phenix_sdk::Project(event)),
        Projection::Exact => quote!(::phenix_sdk::Exact(event)),
        Projection::Projected => quote!(event),
    };
    let call = quote!(self.#name(context, #payload_arg));
    let call = if method.sig.asyncness.is_some() {
        quote!(::phenix_sdk::block_on_static(#call))
    } else {
        call
    };
    let result = if returns_result(&method.sig.output) {
        quote!((#call).map(|_| ()).map_err(|error| {
            Box::new(::std::io::Error::other(error.to_string()))
                as Box<dyn ::std::error::Error + Send + Sync>
        }))
    } else {
        quote!({
            #call;
            Ok(())
        })
    };

    Ok(quote! {
        if listener == #listener {
            let event = match #decode {
                Ok(event) => event,
                Err(error) => return Some(Err(error)),
            };
            return Some(#result);
        }
    })
}

fn export_context(argument: &FnArg) -> Option<ExportContext> {
    if is_named_ref(argument, "CallContext") {
        return Some(ExportContext::Call);
    }
    if is_named_ref(argument, "PluginContext") {
        return Some(ExportContext::Plugin);
    }
    None
}

fn is_named_ref(argument: &FnArg, name: &str) -> bool {
    let FnArg::Typed(argument) = argument else {
        return false;
    };
    let Type::Reference(reference) = argument.ty.as_ref() else {
        return false;
    };
    matches!(reference.elem.as_ref(), Type::Path(path)
        if path.path.segments.last().is_some_and(|segment| segment.ident == name))
}

fn returns_result(output: &ReturnType) -> bool {
    matches!(output, ReturnType::Type(_, ty) if type_ends_with(ty, "Result"))
}

fn success_type(output: &ReturnType) -> Option<&Type> {
    let ReturnType::Type(_, ty) = output else {
        return None;
    };
    let Type::Path(path) = ty.as_ref() else {
        return Some(ty);
    };
    let Some(segment) = path.path.segments.last() else {
        return Some(ty);
    };
    if segment.ident != "Result" {
        return Some(ty);
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.first().and_then(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

fn type_ends_with(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path)
        if path.path.segments.last().is_some_and(|segment| segment.ident == name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn mutable_export_receiver_has_a_targeted_diagnostic() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(export("fixture.api@1"))]
                    fn run(&mut self, _request: Request) -> Response {
                        todo!()
                    }
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("exported component methods must use &self"));
    }
}
