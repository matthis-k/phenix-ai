use crate::interface_attr::validate_interface_id;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse::Parser, parse_quote, punctuated::Punctuated, Attribute, Expr, Fields, FnArg,
    GenericArgument, Ident, ImplItem, ItemImpl, ItemStruct, LitStr, Meta, Path, PathArguments,
    ReturnType, Signature, Token, Type,
};

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "component attributes do not accept root arguments yet",
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
        "#[phenix_sdk::component] applies to a component struct or inherent impl",
    ))
}

fn expand_struct(item: ItemStruct) -> syn::Result<TokenStream> {
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "Phenix components must have one concrete static definition",
        ));
    }
    let mut item = item;
    let contributions = component_fields(&mut item)?;
    let name = &item.ident;
    let imports = contributions.imports.iter().map(|import| {
        let field = &import.field;
        let ty = &import.ty;
        match import.authority.as_ref() {
            Some(authority) => quote! {
                ::phenix_sdk::StaticComponentImport::with_authority::<#ty>(
                    stringify!(#field),
                    #authority,
                )
            },
            None => quote! {
                ::phenix_sdk::StaticComponentImport::of::<#ty>(stringify!(#field))
            },
        }
    });
    let hosts = contributions.hosts.iter().map(|host| {
        let field = &host.field;
        let ty = &host.ty;
        match host.authority.as_ref() {
            Some(authority) => quote! {
                ::phenix_sdk::StaticComponentHost::with_authority::<#ty>(
                    stringify!(#field),
                    #authority,
                )
            },
            None => quote! {
                ::phenix_sdk::StaticComponentHost::of::<#ty>(stringify!(#field))
            },
        }
    });
    let events = contributions.events.iter().map(|event| {
        let field = &event.field;
        let ty = &event.ty;
        let id = &event.event;
        quote! {
            ::phenix_sdk::StaticComponentEvent::of::<#ty>(#id, stringify!(#field))
        }
    });

    Ok(quote! {
        #item

        impl ::phenix_sdk::StaticComponentDefinition for #name {}

        impl ::phenix_sdk::StaticComponentImports for #name {
            fn imports() -> Vec<::phenix_sdk::StaticComponentImport> {
                vec![#(#imports),*]
            }

            fn hosts() -> Vec<::phenix_sdk::StaticComponentHost> {
                vec![#(#hosts),*]
            }

            fn events() -> Vec<::phenix_sdk::StaticComponentEvent> {
                vec![#(#events),*]
            }
        }
    })
}

#[derive(Default)]
struct ComponentFieldContributions {
    imports: Vec<ImportContribution>,
    hosts: Vec<ImportContribution>,
    events: Vec<EventFieldContribution>,
}

struct ImportContribution {
    field: Ident,
    ty: Type,
    authority: Option<Expr>,
}

struct EventFieldContribution {
    field: Ident,
    ty: Type,
    event: LitStr,
}

enum FieldRole {
    Import(ImportContribution),
    Host(ImportContribution),
    Event(EventFieldContribution),
}

fn component_fields(item: &mut ItemStruct) -> syn::Result<ComponentFieldContributions> {
    let Fields::Named(fields) = &mut item.fields else {
        return Ok(ComponentFieldContributions::default());
    };
    let mut contributions = ComponentFieldContributions::default();

    for field in &mut fields.named {
        let mut retained = Vec::new();
        let mut contribution = None;
        for attribute in std::mem::take(&mut field.attrs) {
            if !attribute.path().is_ident("phenix") {
                retained.push(attribute);
                continue;
            }
            if contribution.is_some() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "a component field may have only one Phenix role",
                ));
            }
            let Meta::List(meta) = &attribute.meta else {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "component field attributes must use #[phenix(...)] syntax",
                ));
            };
            let arguments =
                Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
            let name = field.ident.clone().expect("named field has an identifier");
            contribution = Some(match arguments.first() {
                Some(Meta::Path(path)) if path.is_ident("import") => {
                    FieldRole::Import(ImportContribution {
                        field: name,
                        ty: field.ty.clone(),
                        authority: parse_field_authority(&arguments, "import")?,
                    })
                }
                Some(Meta::Path(path)) if path.is_ident("host") => {
                    FieldRole::Host(ImportContribution {
                        field: name,
                        ty: field.ty.clone(),
                        authority: parse_field_authority(&arguments, "host")?,
                    })
                }
                Some(Meta::List(event)) if arguments.len() == 1 && event.path.is_ident("event") => {
                    let id = syn::parse2::<LitStr>(event.tokens.clone())?;
                    validate_event_id(&id.value())
                        .map_err(|error| syn::Error::new_spanned(&id, error))?;
                    FieldRole::Event(EventFieldContribution {
                        field: name,
                        ty: field.ty.clone(),
                        event: id,
                    })
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "unsupported component field contribution",
                    ));
                }
            });
        }
        field.attrs = retained;
        if let Some(contribution) = contribution {
            match contribution {
                FieldRole::Import(value) => contributions.imports.push(value),
                FieldRole::Host(value) => contributions.hosts.push(value),
                FieldRole::Event(value) => contributions.events.push(value),
            }
        }
    }

    Ok(contributions)
}

fn parse_field_authority(
    arguments: &Punctuated<Meta, Token![,]>,
    kind: &'static str,
) -> syn::Result<Option<Expr>> {
    let mut authority = None;
    for argument in arguments.iter().skip(1) {
        match argument {
            Meta::NameValue(value) if value.path.is_ident("authority") => {
                if authority.is_some() {
                    return Err(syn::Error::new_spanned(
                        value,
                        format!("duplicate {kind} authority modifier"),
                    ));
                }
                authority = Some(value.value.clone());
            }
            argument => {
                return Err(syn::Error::new_spanned(
                    argument,
                    format!("{kind} modifiers must use authority = <expression>"),
                ));
            }
        }
    }
    Ok(authority)
}

fn expand_impl(mut item: ItemImpl) -> syn::Result<TokenStream> {
    if item.trait_.is_some() || !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item,
            "component behavior must be a non-generic inherent impl",
        ));
    }

    let mut exports = Vec::new();
    let mut layers = Vec::new();
    let mut listeners = Vec::new();
    let mut values = Vec::new();
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
                    "a component method may declare only one Phenix contribution",
                ));
            }
            contribution = Some(parse_method_contribution(&attribute)?);
        }
        method.attrs = retained;
        let Some(contribution) = contribution else {
            continue;
        };
        match contribution {
            MethodContribution::Export(export) => {
                let (request, response) = export_signature_types(&method.sig, true)?;
                let (decoded_request, projection) = component_request_projection(&request)?;
                let context = method
                    .sig
                    .inputs
                    .iter()
                    .nth(1)
                    .and_then(export_context_parameter);
                let has_request = method.sig.inputs.len() > 1 + usize::from(context.is_some());
                exports.push((
                    method.sig.ident.clone(),
                    export,
                    request,
                    response,
                    decoded_request,
                    projection,
                    context,
                    has_request,
                    export_error_type(&method.sig.output)?,
                    method.sig.asyncness.is_some(),
                ));
            }
            MethodContribution::Layer(layer) => {
                validate_layer_signature(&method.sig)?;
                layers.push((method.sig.ident.clone(), layer));
            }
            MethodContribution::Listen(listener) => {
                let payload = listener_payload(method)?;
                listeners.push((method.sig.ident.clone(), listener, payload));
            }
            MethodContribution::Value(value) => {
                validate_value_signature(&method.sig, &value)?;
                let value_type = value_response_type(&method.sig)?;
                values.push((method.sig.ident.clone(), value, value_type));
            }
        }
    }

    let self_ty = &item.self_ty;
    let export_descriptors = exports.iter().map(
        |(method, export, request, response, _, _, _, _, domain_error, _)| {
            export_descriptor(method, export, request, response, domain_error.as_ref())
        },
    );
    let layer_descriptors = layers.iter().map(|(method, layer)| {
        let interface = &layer.interface;
        let priority = &layer.priority;
        let authority = layer
            .authority
            .as_ref()
            .map(|authority| quote!(#authority))
            .unwrap_or_else(|| quote!(::phenix_sdk::Authority::default()));
        quote! {
            ::phenix_sdk::StaticComponentLayer::with_authority::<#interface>(
                stringify!(#method),
                #priority,
                #authority,
            )
        }
    });
    let listener_descriptors = listeners.iter().map(|(method, listener, (payload, projection))| {
        let event = &listener.event;
        match (projection, listener.authority.as_ref()) {
            (ListenerProjection::Projected, Some(authority)) => quote! {
                ::phenix_sdk::StaticComponentListener::with_authority::<#payload>(
                    #event,
                    stringify!(#method),
                    #authority,
                )
            },
            (ListenerProjection::Projected, None) => quote! {
                ::phenix_sdk::StaticComponentListener::of::<#payload>(#event, stringify!(#method))
            },
            (ListenerProjection::Exact, Some(authority)) => quote! {
                ::phenix_sdk::StaticComponentListener::exact_with_authority::<#payload>(
                    #event,
                    stringify!(#method),
                    #authority,
                )
            },
            (ListenerProjection::Exact, None) => quote! {
                ::phenix_sdk::StaticComponentListener::exact::<#payload>(#event, stringify!(#method))
            },
        }
    });
    let value_descriptors = values.iter().map(|(method, value, value_type)| {
        let id = &value.id;
        let public = value.public;
        quote! {
            ::phenix_sdk::StaticComponentValue::of::<#value_type>(
                #id, stringify!(#method), #public,
            )
        }
    });
    let dispatch_arms = exports.iter().map(
        |(
            method,
            export,
            _request,
            _response,
            decoded_request,
            projection,
            context,
            has_request,
            domain_error,
            is_async,
        )| {
            let interface = export.interface_expression();
            if *is_async {
                return quote! {
                    {
                        let interface = #interface;
                        if service.as_str() == interface.as_str() {
                            return Err(format!(
                                "async component export {} requires an async runtime adapter",
                                interface.as_str(),
                            ));
                        }
                    }
                };
            }
            let request = match projection {
                ComponentRequestProjection::Projected => quote!(request),
                ComponentRequestProjection::ExplicitProject => {
                    quote!(::phenix_sdk::Project(request))
                }
                ComponentRequestProjection::Exact => quote!(::phenix_sdk::Exact(request)),
            };
            let call = match (*context, *has_request) {
                (Some(ExportContext::Call), true) => {
                    quote!(self.#method(&plugin_context.call, #request))
                }
                (Some(ExportContext::Call), false) => quote!(self.#method(&plugin_context.call)),
                (Some(ExportContext::Plugin), true) => {
                    quote!(self.#method(&plugin_context, #request))
                }
                (Some(ExportContext::Plugin), false) => quote!(self.#method(&plugin_context)),
                (None, true) => quote!(self.#method(#request)),
                (None, false) => quote!(self.#method()),
            };
            let request_binding = if *has_request {
                quote!(request)
            } else {
                quote!(_request)
            };
            let handler = if domain_error.is_some() {
                quote!(|#request_binding: #decoded_request| #call)
            } else {
                quote!(|#request_binding: #decoded_request| Ok::<_, String>(#call))
            };
            let dispatch = match projection {
                ComponentRequestProjection::Exact => quote!(::phenix_sdk::dispatch_exact_provider),
                ComponentRequestProjection::Projected
                | ComponentRequestProjection::ExplicitProject => {
                    quote!(::phenix_sdk::dispatch_projected_provider)
                }
            };

            quote! {
                {
                    let interface = #interface;
                    if service.as_str() == interface.as_str() {
                        let plugin_context = ::phenix_sdk::PluginContext::new(host, (), (), ());
                        return #dispatch(host, &interface, input, #handler);
                    }
                }
            }
        },
    );

    Ok(quote! {
        #item

        impl ::phenix_sdk::StaticComponentBehavior for #self_ty {
            fn exports() -> Vec<::phenix_sdk::StaticComponentExport> {
                vec![#(#export_descriptors),*]
            }

            fn layers() -> Vec<::phenix_sdk::StaticComponentLayer> {
                vec![#(#layer_descriptors),*]
            }

            fn listeners() -> Vec<::phenix_sdk::StaticComponentListener> {
                vec![#(#listener_descriptors),*]
            }

            fn values() -> Vec<::phenix_sdk::StaticComponentValue> {
                vec![#(#value_descriptors),*]
            }
        }

        impl ::phenix_sdk::StaticComponentDispatch for #self_ty {
            fn dispatch(
                &self,
                service: &::phenix_sdk::__phenix_plugin::ServiceId,
                input: &[u8],
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                #(#dispatch_arms)*
                Err(format!("unsupported component service: {service}"))
            }
        }
    })
}

#[derive(Clone, Copy)]
enum ComponentRequestProjection {
    Projected,
    ExplicitProject,
    Exact,
}

#[derive(Clone, Copy)]
enum ExportContext {
    Call,
    Plugin,
}

fn component_request_projection(request: &Type) -> syn::Result<(Type, ComponentRequestProjection)> {
    let Type::Path(path) = request else {
        return Ok((request.clone(), ComponentRequestProjection::Projected));
    };
    let Some(segment) = path.path.segments.last() else {
        return Ok((request.clone(), ComponentRequestProjection::Projected));
    };
    let projection = if segment.ident == "Exact" {
        ComponentRequestProjection::Exact
    } else if segment.ident == "Project" {
        ComponentRequestProjection::ExplicitProject
    } else {
        return Ok((request.clone(), ComponentRequestProjection::Projected));
    };
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "Project and Exact component request wrappers require exactly one payload type",
        ));
    };
    if arguments.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            arguments,
            "Project and Exact component request wrappers require exactly one payload type",
        ));
    }
    let Some(GenericArgument::Type(inner)) = arguments.args.first() else {
        return Err(syn::Error::new_spanned(
            arguments,
            "Project and Exact component request wrappers require exactly one payload type",
        ));
    };
    Ok((inner.clone(), projection))
}

enum MethodContribution {
    Export(ExportContribution),
    Layer(LayerContribution),
    Listen(ListenContribution),
    Value(ValueContribution),
}

struct LayerContribution {
    interface: Path,
    priority: Expr,
    authority: Option<Expr>,
}

struct ListenContribution {
    event: LitStr,
    authority: Option<Expr>,
}

fn validate_layer_signature(signature: &Signature) -> syn::Result<()> {
    if !signature.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &signature.generics,
            "component layers cannot be generic",
        ));
    }
    let Some(FnArg::Receiver(receiver)) = signature.inputs.first() else {
        return Err(syn::Error::new_spanned(
            signature,
            "component layers require a borrowed self receiver",
        ));
    };
    if receiver.reference.is_none() {
        return Err(syn::Error::new_spanned(
            receiver,
            "component layers require a borrowed self receiver",
        ));
    }
    Ok(())
}

fn listener_payload(method: &syn::ImplItemFn) -> syn::Result<(Type, ListenerProjection)> {
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            "component listeners must be non-generic",
        ));
    }
    let mut inputs = method.sig.inputs.iter();
    let Some(FnArg::Receiver(receiver)) = inputs.next() else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "component listeners require a borrowed self receiver, &EventContext, and one typed event payload",
        ));
    };
    if receiver.reference.is_none() {
        return Err(syn::Error::new_spanned(
            receiver,
            "component listeners require a borrowed self receiver",
        ));
    }
    let Some(context) = inputs.next() else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "component listeners require &EventContext followed by one typed event payload",
        ));
    };
    if !is_event_context_parameter(context) {
        return Err(syn::Error::new_spanned(
            context,
            "component listeners require &EventContext before the event payload",
        ));
    }
    let Some(FnArg::Typed(payload)) = inputs.next() else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "component listeners require &EventContext followed by one typed event payload",
        ));
    };
    if inputs.next().is_some() {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "component listeners require &EventContext followed by exactly one typed event payload",
        ));
    }
    let payload = (*payload.ty).clone();
    listener_projection(payload)
}

fn is_event_context_parameter(argument: &FnArg) -> bool {
    let FnArg::Typed(argument) = argument else {
        return false;
    };
    let Type::Reference(reference) = argument.ty.as_ref() else {
        return false;
    };
    reference.mutability.is_none()
        && matches!(reference.elem.as_ref(), Type::Path(context)
            if context.qself.is_none()
                && context.path.segments.last().is_some_and(|segment| segment.ident == "EventContext"))
}

#[derive(Clone, Copy, Debug)]
enum ListenerProjection {
    Projected,
    Exact,
}

fn listener_projection(payload: Type) -> syn::Result<(Type, ListenerProjection)> {
    let Type::Path(path) = &payload else {
        return Ok((payload, ListenerProjection::Projected));
    };
    let Some(segment) = path.path.segments.last() else {
        return Ok((payload, ListenerProjection::Projected));
    };
    let projection = if segment.ident == "Exact" {
        ListenerProjection::Exact
    } else if segment.ident == "Project" {
        ListenerProjection::Projected
    } else {
        return Ok((payload, ListenerProjection::Projected));
    };
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "Project and Exact listener wrappers require exactly one payload type",
        ));
    };
    if arguments.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            arguments,
            "Project and Exact listener wrappers require exactly one payload type",
        ));
    }
    let Some(GenericArgument::Type(inner)) = arguments.args.first() else {
        return Err(syn::Error::new_spanned(
            arguments,
            "Project and Exact listener wrappers require exactly one payload type",
        ));
    };
    Ok((inner.clone(), projection))
}

struct ValueContribution {
    id: LitStr,
    public: bool,
}

fn parse_method_contribution(attribute: &Attribute) -> syn::Result<MethodContribution> {
    let Meta::List(meta) = &attribute.meta else {
        return Err(syn::Error::new_spanned(
            attribute,
            "component method attributes must use #[phenix(...)] syntax",
        ));
    };
    let outer = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let Some(Meta::List(kind)) = outer.first() else {
        return Err(syn::Error::new_spanned(
            attribute,
            "component method contribution must begin with a contribution",
        ));
    };

    if kind.path.is_ident("export") {
        return parse_export(attribute).map(MethodContribution::Export);
    }
    if kind.path.is_ident("layer") {
        return parse_layer(attribute).map(MethodContribution::Layer);
    }
    if kind.path.is_ident("listen") {
        return parse_listener(attribute).map(MethodContribution::Listen);
    }
    if kind.path.is_ident("value") {
        return parse_value(attribute).map(MethodContribution::Value);
    }

    Err(syn::Error::new_spanned(
        &kind.path,
        "unsupported component method contribution",
    ))
}

pub(crate) struct ExportContribution {
    interface: ExportInterface,
    public: bool,
    terminal: bool,
    priority: Option<Expr>,
    authority: Option<Expr>,
}

impl ExportContribution {
    pub(crate) fn interface_expression(&self) -> TokenStream {
        self.interface.expression()
    }
}

enum ExportInterface {
    Literal(LitStr),
    Marker(Box<Type>),
}

impl ExportInterface {
    fn expression(&self) -> TokenStream {
        match self {
            Self::Literal(id) => quote! {
                ::phenix_sdk::__phenix_plugin::InterfaceId::parse(#id)
                    .expect("component export contains a valid static interface id")
            },
            Self::Marker(marker) => quote! {
                <#marker as ::phenix_sdk::InterfaceMarker>::interface_id()
            },
        }
    }
}

pub(crate) fn export_descriptor(
    method: &Ident,
    export: &ExportContribution,
    request: &Type,
    response: &Type,
    domain_error: Option<&Type>,
) -> TokenStream {
    let interface = export.interface.expression();
    let public = export.public;
    let terminal = export.terminal;
    let priority = export
        .priority
        .as_ref()
        .map(|priority| quote!(#priority))
        .unwrap_or_else(|| quote!(0));
    let authority = export
        .authority
        .as_ref()
        .map(|authority| quote!(#authority))
        .unwrap_or_else(|| quote!(::phenix_sdk::__phenix_plugin::Authority::default()));
    let schema = match domain_error {
        Some(domain_error) => quote! {
            ::phenix_sdk::__phenix_plugin::InterfaceSchema::fallible_of::<
                #request,
                #response,
                #domain_error,
            >()
        },
        None => quote! {
            ::phenix_sdk::__phenix_plugin::InterfaceSchema::of::<#request, #response>()
        },
    };
    quote! {
        ::phenix_sdk::StaticComponentExport {
            interface: #interface,
            schema: #schema,
            method: stringify!(#method),
            public: #public,
            terminal: #terminal,
            priority: #priority,
            required_authority: #authority,
        }
    }
}

pub(crate) fn export_signature_types(
    signature: &Signature,
    requires_receiver: bool,
) -> syn::Result<(Type, Type)> {
    if !signature.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &signature.generics,
            "exports must be non-generic",
        ));
    }

    let mut inputs = signature.inputs.iter();
    if requires_receiver {
        let Some(FnArg::Receiver(receiver)) = inputs.next() else {
            return Err(syn::Error::new_spanned(
                signature,
                "component exports require a self receiver",
            ));
        };
        if receiver.reference.is_none() {
            return Err(syn::Error::new_spanned(
                receiver,
                "component exports require a borrowed self receiver",
            ));
        }
    }

    if inputs
        .clone()
        .next()
        .and_then(export_context_parameter)
        .is_some()
    {
        inputs.next();
    }

    let request = match inputs.next() {
        None => parse_quote!(()),
        Some(FnArg::Typed(request)) => (*request.ty).clone(),
        Some(FnArg::Receiver(_)) => {
            return Err(syn::Error::new_spanned(
                signature,
                "export signatures may contain only one self receiver",
            ));
        }
    };
    if inputs.next().is_some() {
        return Err(syn::Error::new_spanned(
            signature,
            "exports accept at most one typed request parameter after an optional &CallContext or &PluginContext",
        ));
    }

    Ok((request, export_response_type(&signature.output)?))
}

fn export_context_parameter(argument: &FnArg) -> Option<ExportContext> {
    if is_call_context_parameter(argument) {
        return Some(ExportContext::Call);
    }
    if is_plugin_context_parameter(argument) {
        return Some(ExportContext::Plugin);
    }
    None
}

pub(crate) fn is_call_context_parameter(argument: &FnArg) -> bool {
    is_named_context_parameter(argument, "CallContext")
}

fn is_plugin_context_parameter(argument: &FnArg) -> bool {
    is_named_context_parameter(argument, "PluginContext")
}

fn is_named_context_parameter(argument: &FnArg, name: &str) -> bool {
    let FnArg::Typed(argument) = argument else {
        return false;
    };
    let Type::Reference(reference) = argument.ty.as_ref() else {
        return false;
    };
    let Type::Path(context) = reference.elem.as_ref() else {
        return false;
    };

    context.qself.is_none()
        && context
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == name)
}

fn export_response_type(output: &ReturnType) -> syn::Result<Type> {
    let ReturnType::Type(_, output) = output else {
        return Ok(parse_quote!(()));
    };
    let Type::Path(path) = output.as_ref() else {
        return Ok((**output).clone());
    };
    let Some(segment) = path.path.segments.last() else {
        return Ok((**output).clone());
    };
    if segment.ident != "Result" {
        return Ok((**output).clone());
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            output,
            "export Result response must name its success type",
        ));
    };
    let Some(GenericArgument::Type(response)) = arguments.args.first() else {
        return Err(syn::Error::new_spanned(
            output,
            "export Result response must name its success type",
        ));
    };
    Ok(response.clone())
}

pub(crate) fn export_error_type(output: &ReturnType) -> syn::Result<Option<Type>> {
    let ReturnType::Type(_, output) = output else {
        return Ok(None);
    };
    let Type::Path(path) = output.as_ref() else {
        return Ok(None);
    };
    let Some(segment) = path.path.segments.last() else {
        return Ok(None);
    };
    if segment.ident != "Result" {
        return Ok(None);
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            output,
            "export Result response must name its domain error type",
        ));
    };
    let Some(GenericArgument::Type(error)) = arguments.args.iter().nth(1) else {
        return Err(syn::Error::new_spanned(
            output,
            "export Result response must name its domain error type",
        ));
    };
    Ok(Some(error.clone()))
}

pub(crate) fn value_response_type(signature: &Signature) -> syn::Result<Type> {
    export_response_type(&signature.output)
}

pub(crate) fn parse_export(attribute: &Attribute) -> syn::Result<ExportContribution> {
    let Meta::List(meta) = &attribute.meta else {
        return Err(syn::Error::new_spanned(
            attribute,
            "component method attributes must use #[phenix(...)] syntax",
        ));
    };
    let arguments = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let mut arguments = arguments.into_iter();
    let Some(Meta::List(export)) = arguments.next() else {
        return Err(syn::Error::new_spanned(
            attribute,
            "component method contribution must begin with export(...)",
        ));
    };
    if !export.path.is_ident("export") {
        return Err(syn::Error::new_spanned(
            export.path,
            "unsupported component method contribution",
        ));
    }

    let interface = if let Ok(id) = syn::parse2::<LitStr>(export.tokens.clone()) {
        validate_interface_id(&id.value()).map_err(|error| syn::Error::new_spanned(&id, error))?;
        ExportInterface::Literal(id)
    } else {
        ExportInterface::Marker(Box::new(syn::parse2::<Type>(export.tokens)?))
    };

    let (public, terminal, priority, authority) = parse_export_modifiers(arguments)?;
    Ok(ExportContribution {
        interface,
        public,
        terminal,
        priority,
        authority,
    })
}

fn parse_export_modifiers(
    arguments: impl IntoIterator<Item = Meta>,
) -> syn::Result<(bool, bool, Option<Expr>, Option<Expr>)> {
    let mut public = false;
    let mut terminal = false;
    let mut priority = None;
    let mut authority = None;

    for argument in arguments {
        match argument {
            Meta::Path(path) if path.is_ident("public") => {
                if public {
                    return Err(syn::Error::new_spanned(
                        path,
                        "duplicate export public modifier",
                    ));
                }
                public = true;
            }
            Meta::Path(path) if path.is_ident("terminal") => {
                if terminal {
                    return Err(syn::Error::new_spanned(
                        path,
                        "duplicate export terminal modifier",
                    ));
                }
                terminal = true;
            }
            Meta::NameValue(value) if value.path.is_ident("priority") => {
                if priority.is_some() {
                    return Err(syn::Error::new_spanned(
                        value,
                        "duplicate export priority modifier",
                    ));
                }
                priority = Some(value.value);
            }
            Meta::NameValue(value) if value.path.is_ident("authority") => {
                if authority.is_some() {
                    return Err(syn::Error::new_spanned(
                        value,
                        "duplicate export authority modifier",
                    ));
                }
                authority = Some(value.value);
            }
            Meta::Path(path) => {
                return Err(syn::Error::new_spanned(path, "unsupported export modifier"));
            }
            argument => {
                return Err(syn::Error::new_spanned(
                    argument,
                    "export modifiers must use public, terminal, priority = <expression>, or authority = <expression>",
                ));
            }
        }
    }

    Ok((public, terminal, priority, authority))
}

fn parse_layer(attribute: &Attribute) -> syn::Result<LayerContribution> {
    let Meta::List(meta) = &attribute.meta else {
        unreachable!("method contribution parser already validated list syntax")
    };
    let outer = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    if outer.len() != 1 {
        return Err(syn::Error::new_spanned(
            attribute,
            "layer modifiers belong inside layer(...) metadata",
        ));
    }
    let Some(Meta::List(layer)) = outer.first() else {
        unreachable!("method contribution parser already validated nested list syntax")
    };
    let arguments = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(layer.tokens.clone())?;
    let mut arguments = arguments.into_iter();
    let Some(Meta::Path(interface)) = arguments.next() else {
        return Err(syn::Error::new_spanned(
            layer,
            "layer requires an interface marker type",
        ));
    };
    let mut priority = None;
    let mut authority = None;
    for argument in arguments {
        let Meta::NameValue(value) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "layer metadata must use priority = <expression> or authority = <expression>",
            ));
        };
        if value.path.is_ident("priority") {
            if priority.is_some() {
                return Err(syn::Error::new_spanned(value, "duplicate layer priority"));
            }
            priority = Some(value.value);
        } else if value.path.is_ident("authority") {
            if authority.is_some() {
                return Err(syn::Error::new_spanned(value, "duplicate layer authority"));
            }
            authority = Some(value.value);
        } else {
            return Err(syn::Error::new_spanned(
                value.path,
                "unsupported layer metadata",
            ));
        }
    }
    let Some(priority) = priority else {
        return Err(syn::Error::new_spanned(
            layer,
            "layer requires priority = <expression>",
        ));
    };
    Ok(LayerContribution {
        interface,
        priority,
        authority,
    })
}

fn parse_listener(attribute: &Attribute) -> syn::Result<ListenContribution> {
    let Meta::List(meta) = &attribute.meta else {
        unreachable!("method contribution parser already validated list syntax")
    };
    let outer = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let Some(Meta::List(listener)) = outer.first() else {
        unreachable!("method contribution parser already validated nested list syntax")
    };
    let event = syn::parse2::<LitStr>(listener.tokens.clone())?;
    validate_event_id(&event.value()).map_err(|error| syn::Error::new_spanned(&event, error))?;
    let mut authority = None;
    for modifier in outer.iter().skip(1) {
        let Meta::NameValue(value) = modifier else {
            return Err(syn::Error::new_spanned(
                modifier,
                "listener modifiers must use authority = <expression>",
            ));
        };
        if !value.path.is_ident("authority") {
            return Err(syn::Error::new_spanned(
                &value.path,
                "unsupported listener modifier",
            ));
        }
        if authority.is_some() {
            return Err(syn::Error::new_spanned(
                value,
                "duplicate listener authority modifier",
            ));
        }
        authority = Some(value.value.clone());
    }
    Ok(ListenContribution { event, authority })
}

fn parse_value(attribute: &Attribute) -> syn::Result<ValueContribution> {
    let Meta::List(meta) = &attribute.meta else {
        unreachable!("method contribution parser already validated list syntax")
    };
    let arguments = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let mut arguments = arguments.into_iter();
    let Some(Meta::List(value)) = arguments.next() else {
        unreachable!("method contribution parser already validated nested list syntax")
    };
    let id = syn::parse2::<LitStr>(value.tokens)?;
    validate_interface_id(&id.value()).map_err(|error| syn::Error::new_spanned(&id, error))?;
    let public = parse_public_modifier(arguments, "value")?;
    Ok(ValueContribution { id, public })
}

fn validate_value_signature(signature: &Signature, value: &ValueContribution) -> syn::Result<()> {
    if !value.public {
        return Ok(());
    }

    if !signature.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &signature.generics,
            "public component values cannot be generic",
        ));
    }

    let mut inputs = signature.inputs.iter();
    let Some(FnArg::Receiver(receiver)) = inputs.next() else {
        return Err(syn::Error::new_spanned(
            signature,
            "public component values require a shared self receiver",
        ));
    };
    if receiver.reference.is_none() || receiver.mutability.is_some() {
        return Err(syn::Error::new_spanned(
            receiver,
            "public component values cannot consume or mutably borrow self",
        ));
    }

    if let Some(context) = inputs.next() {
        if !is_read_context_parameter(context) {
            return Err(syn::Error::new_spanned(
                context,
                "public component values accept only an optional &ReadContext after &self",
            ));
        }
    }
    if inputs.next().is_some() {
        return Err(syn::Error::new_spanned(
            signature,
            "public component values accept only an optional &ReadContext after &self",
        ));
    }

    Ok(())
}

fn is_read_context_parameter(argument: &FnArg) -> bool {
    let FnArg::Typed(argument) = argument else {
        return false;
    };
    let Type::Reference(reference) = argument.ty.as_ref() else {
        return false;
    };
    reference.mutability.is_none()
        && matches!(reference.elem.as_ref(), Type::Path(context)
            if context.qself.is_none()
                && context.path.segments.last().is_some_and(|segment| segment.ident == "ReadContext"))
}

fn parse_public_modifier(
    arguments: impl IntoIterator<Item = Meta>,
    kind: &'static str,
) -> syn::Result<bool> {
    let mut public = false;
    for argument in arguments {
        let Meta::Path(path) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                format!("{kind} modifiers must be bare flags"),
            ));
        };
        if !path.is_ident("public") {
            return Err(syn::Error::new_spanned(
                path,
                format!("unsupported {kind} modifier"),
            ));
        }
        if public {
            return Err(syn::Error::new_spanned(
                path,
                format!("duplicate {kind} public modifier"),
            ));
        }
        public = true;
    }
    Ok(public)
}

fn validate_event_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("event id must not be empty");
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
    }) {
        Ok(())
    } else {
        Err("event id contains unsupported characters")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn listener_accepts_async_methods() {
        let method: syn::ImplItemFn = parse_quote! {
            async fn changed(&mut self, _context: &EventContext, event: String) {}
        };

        let (payload, projection) =
            listener_payload(&method).expect("async listener should be supported");
        assert_eq!(quote!(#payload).to_string(), "String");
        assert!(matches!(projection, ListenerProjection::Projected));
    }

    #[test]
    fn listener_payload_preserves_structural_policy() {
        let projected: syn::ImplItemFn = parse_quote! {
            fn changed(&mut self, _context: &EventContext, event: Project<Event>) {}
        };
        let exact: syn::ImplItemFn = parse_quote! {
            fn changed(&mut self, _context: &EventContext, event: Exact<Event>) {}
        };

        let (payload, projection) = listener_payload(&projected).unwrap();
        assert_eq!(quote!(#payload).to_string(), "Event");
        assert!(matches!(projection, ListenerProjection::Projected));

        let (payload, projection) = listener_payload(&exact).unwrap();
        assert_eq!(quote!(#payload).to_string(), "Event");
        assert!(matches!(projection, ListenerProjection::Exact));
    }

    #[test]
    fn listener_rejects_malformed_structural_wrapper() {
        let malformed: syn::ImplItemFn = parse_quote! {
            fn changed(&mut self, _context: &EventContext, event: Exact<Event, Unexpected>) {}
        };

        let error = match listener_payload(&malformed) {
            Err(error) => error,
            Ok(_) => panic!("malformed structural listener wrapper should fail"),
        };
        assert!(error
            .to_string()
            .contains("require exactly one payload type"));
    }

    #[test]
    fn listener_rejects_missing_event_context() {
        let method: syn::ImplItemFn = parse_quote! {
            fn changed(&mut self, event: String) {}
        };

        let error = match listener_payload(&method) {
            Err(error) => error,
            Ok(_) => panic!("listener without event context should fail"),
        };
        assert!(error.to_string().contains("require &EventContext"));
    }

    #[test]
    fn listener_rejects_owned_self() {
        let method: syn::ImplItemFn = parse_quote! {
            fn changed(self, _context: &EventContext, event: String) {}
        };

        let error = match listener_payload(&method) {
            Err(error) => error,
            Ok(_) => panic!("owned listener receiver should fail"),
        };
        assert_eq!(
            error.to_string(),
            "component listeners require a borrowed self receiver"
        );
    }

    #[test]
    fn public_value_rejects_generic_methods() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(value("fixture.status@1"), public)]
                    fn status<T>(&self) -> T {
                        todo!()
                    }
                }
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "public component values cannot be generic"
        );
    }

    #[test]
    fn public_value_rejects_request_parameters() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(value("fixture.status@1"), public)]
                    fn status(&self, request: Request) -> Status {
                        todo!()
                    }
                }
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "public component values accept only an optional &ReadContext after &self"
        );
    }

    #[test]
    fn public_value_accepts_read_context() {
        expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(value("fixture.status@1"), public)]
                    fn status(&self, _context: &ReadContext) -> Status {
                        todo!()
                    }
                }
            },
        )
        .expect("public values may use read-only context");
    }

    #[test]
    fn public_value_rejects_static_methods() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(value("fixture.status@1"), public)]
                    fn status() -> Status {
                        todo!()
                    }
                }
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "public component values require a shared self receiver"
        );
    }

    #[test]
    fn component_struct_lowers_to_static_definition() {
        let output = expand(
            TokenStream::new(),
            quote!(
                struct Api;
            ),
        )
        .unwrap();
        let output = output.to_string();

        assert!(output.contains("StaticComponentDefinition"));
        assert!(output.contains("Api"));
    }

    #[test]
    fn component_struct_lowers_typed_import_fields() {
        let output = expand(
            TokenStream::new(),
            quote! {
                struct Api {
                    #[phenix(import)]
                    models: Required<Call<Models, Request, Response>>,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticComponentImports for Api"));
        assert!(output.contains("StaticComponentImport :: of"));
        assert!(output.contains("Required < Call < Models , Request , Response > >"));
        assert!(output.contains("stringify ! (models)"));
        assert!(!output.contains("phenix (import"));
    }

    #[test]
    fn component_struct_lowers_host_authority() {
        let output = expand(
            TokenStream::new(),
            quote! {
                struct Api {
                    #[phenix(host, authority = Authority::default())]
                    clock: Host<Clock>,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticComponentHost :: with_authority"));
        assert!(output.contains("Host < Clock >"));
        assert!(output.contains("Authority :: default"));
        assert!(!output.contains("phenix (host"));
    }

    #[test]
    fn component_struct_rejects_duplicate_field_roles() {
        let error = expand(
            TokenStream::new(),
            quote! {
                struct Api {
                    #[phenix(import)]
                    #[phenix(import)]
                    models: Required<Call<Models, Request, Response>>,
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("only one Phenix role"));
    }

    #[test]
    fn component_struct_rejects_unknown_field_contributions() {
        let error = expand(
            TokenStream::new(),
            quote!(
                struct Api {
                    #[phenix(unknown)]
                    value: String,
                }
            ),
        )
        .unwrap_err();

        assert!(error.to_string().contains("unsupported component field"));
    }

    #[test]
    fn component_struct_rejects_invalid_event_id() {
        let error = expand(
            TokenStream::new(),
            quote! {
                struct Api {
                    #[phenix(event("fixture completed"))]
                    completed: Emit<Completed>,
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("event id contains unsupported characters"));
    }

    #[test]
    fn component_impl_lowers_all_documented_behavior_metadata() {
        let output = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(
                        export("fixture.api.run@1"),
                        public,
                        priority = 37,
                        authority = Authority::default()
                    )]
                    fn run(&mut self, request: Request) -> Result<Response, Error> {
                        todo!()
                    }

                    #[phenix(layer(Models, priority = 17))]
                    fn policy(&mut self) {}

                    #[phenix(listen("fixture.completed"))]
                    fn completed(&mut self, _context: &EventContext, _event: Completed) {}

                    #[phenix(value("fixture.status@1"), public)]
                    fn status(&self) -> Status {
                        todo!()
                    }
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticComponentBehavior"));
        assert!(output.contains("fixture.api.run@1"));
        assert!(output.contains("priority : 37"));
        assert!(output.contains("required_authority : Authority :: default"));
        assert!(
            output.contains("InterfaceSchema :: fallible_of :: < Request , Response , Error , >")
        );
        assert!(output.contains("StaticComponentLayer :: with_authority :: < Models >"));
        assert!(output.contains("fixture.completed"));
        assert!(output.contains("fixture.status@1"));
        assert!(output.contains("public : true"));
        assert!(!output.contains("phenix (export"));
        assert!(!output.contains("phenix (layer"));
        assert!(!output.contains("phenix (listen"));
        assert!(!output.contains("phenix (value"));
    }

    #[test]
    fn component_impl_accepts_plugin_context() {
        let output = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(export("fixture.api.run@1"))]
                    fn run(
                        &mut self,
                        _context: &PluginContext<'_, '_, ()>,
                        request: Request,
                    ) -> Response {
                        todo!()
                    }
                }
            },
        )
        .expect("component exports may request plugin context")
        .to_string();

        assert!(output.contains("PluginContext :: new"));
        assert!(output.contains("self . run (& plugin_context , request)"));
    }

    #[test]
    fn component_impl_defaults_export_routing_metadata() {
        let output = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(export(Planning))]
                    fn plan(&mut self) {}
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("priority : 0"));
        assert!(output.contains("Authority :: default"));
    }

    #[test]
    fn component_impl_rejects_duplicate_export_routing_metadata() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(export(Planning), priority = 1, priority = 2)]
                    fn plan(&mut self) {}
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("duplicate export priority"));
    }

    #[test]
    fn component_impl_rejects_layer_without_component_receiver() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(layer(Planning, priority = 1))]
                    fn policy(_request: Request) {}
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("layers require a borrowed self receiver"));
    }

    #[test]
    fn component_impl_accepts_interface_marker_type() {
        let output = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(export(Planning))]
                    fn plan(&mut self) {}
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("Planning as :: phenix_sdk :: InterfaceMarker"));
    }

    #[test]
    fn component_impl_rejects_unversioned_literal_interface() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(export("fixture.api.run"))]
                    fn run(&mut self) {}
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("@version"));
    }

    #[test]
    fn component_impl_rejects_invalid_event_id() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(listen("fixture completed"))]
                    fn completed(&mut self) {}
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("event id contains unsupported"));
    }

    #[test]
    fn component_impl_rejects_mutable_public_value_receiver() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Api {
                    #[phenix(value("fixture.status@1"), public)]
                    fn status(&mut self) -> Status {
                        todo!()
                    }
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("cannot consume or mutably borrow self"));
    }
}
