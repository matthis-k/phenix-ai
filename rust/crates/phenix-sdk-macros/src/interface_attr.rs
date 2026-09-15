use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, ItemStruct, LitStr};

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let id = syn::parse2::<LitStr>(args)?;
    let item = syn::parse2::<ItemStruct>(input)?;
    if !item.generics.params.is_empty() || !matches!(item.fields, Fields::Unit) {
        return Err(syn::Error::new_spanned(
            &item,
            "Phenix interface markers must be non-generic unit structs",
        ));
    }
    validate_interface_id(&id.value()).map_err(|error| syn::Error::new_spanned(&id, error))?;
    let name = &item.ident;

    Ok(quote! {
        #item

        impl ::phenix_sdk::InterfaceMarker for #name {
            fn interface_id() -> ::phenix_sdk::__phenix_plugin::InterfaceId {
                ::phenix_sdk::__phenix_plugin::InterfaceId::parse(#id)
                    .expect("interface attribute contains a valid static interface id")
            }
        }
    })
}

pub(crate) fn validate_interface_id(value: &str) -> Result<(), &'static str> {
    let (identity, version) = value
        .rsplit_once('@')
        .ok_or("interface id must include an @version suffix")?;
    if identity.is_empty() || identity.contains('@') {
        return Err("interface id must contain one non-empty identity and @version suffix");
    }
    if !identity.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
    }) {
        return Err("interface id contains unsupported characters");
    }
    let version = version
        .parse::<u64>()
        .map_err(|_| "interface id version must be a positive integer")?;
    if version == 0 {
        return Err("interface id version must be a positive integer");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn interface_marker_lowers_stable_identity() {
        let output = expand(
            quote!("phenix.models.inference@1"),
            quote!(
                pub struct ModelsInference;
            ),
        )
        .unwrap()
        .to_string();

        assert!(output.contains("InterfaceMarker"));
        assert!(output.contains("phenix.models.inference@1"));
    }

    #[test]
    fn interface_identity_requires_positive_version() {
        assert!(validate_interface_id("phenix.models.inference@1").is_ok());
        assert!(validate_interface_id("phenix.models.inference").is_err());
        assert!(validate_interface_id("phenix.models.inference@0").is_err());
    }
}
