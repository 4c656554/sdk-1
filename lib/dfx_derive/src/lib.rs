extern crate proc_macro;
extern crate proc_macro2;
#[macro_use]
extern crate syn;
extern crate quote;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use std::collections::BTreeSet;
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, Data, DeriveInput, GenericParam, Generics};

#[proc_macro_derive(IDLType)]
pub fn derive_idl_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let generics = add_trait_bounds(input.generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let (ty_body, ser_body) = match input.data {
        Data::Enum(ref data) => enum_from_ast(&name, &data.variants),
        Data::Struct(ref data) => {
            let (ty, idents) = struct_from_ast(&data.fields);
            (ty, serialize_struct(&idents))
        }
        Data::Union(_) => unimplemented!("doesn't derive union type"),
    };
    let gen = quote! {
        impl #impl_generics dfx_info::IDLType for #name #ty_generics #where_clause {
            fn _ty() -> dfx_info::types::Type {
                #ty_body
            }
            fn id() -> dfx_info::types::TypeId { dfx_info::types::TypeId::of::<#name #ty_generics>() }

            fn idl_serialize<__S>(&self, __serializer: __S) -> Result<(), __S::Error>
                where
                __S: dfx_info::Serializer,
                {
                    #ser_body
                }
        }
    };
    //panic!(gen.to_string());
    TokenStream::from(gen)
}

#[inline]
fn idl_hash(id: &str) -> u32 {
    let mut s: u32 = 0;
    for c in id.chars() {
        s = s.wrapping_mul(223).wrapping_add(c as u32);
    }
    s
}

struct Variant {
    ident: syn::Ident,
    hash: u32,
    ty: Tokens,
    members: Vec<Ident>,
}
enum Style {
    Struct,
    Tuple,
    Unit,
}
impl Variant {
    fn style(&self) -> Style {
        if self.members.is_empty() {
            return Style::Unit;
        };
        match self.members[0] {
            Ident::Named(_) => Style::Struct,
            Ident::Unnamed(_) => Style::Tuple,
        }
    }
    fn to_pattern(&self) -> (Tokens, Vec<Tokens>) {
        match self.style() {
            Style::Unit => (quote! {}, Vec::new()),
            Style::Struct => {
                let id: Vec<_> = self.members.iter().map(|ident| ident.to_token()).collect();
                (
                    quote! {
                        {#(ref #id),*}
                    },
                    id,
                )
            }
            Style::Tuple => {
                let id: Vec<_> = self
                    .members
                    .iter()
                    .map(|ident| {
                        let ident = ident.to_string();
                        let var = format!("__field{}", ident.to_string());
                        syn::parse_str(&var).unwrap()
                    })
                    .collect();
                (
                    quote! {
                        (#(ref #id),*)
                    },
                    id,
                )
            }
        }
    }
}

fn enum_from_ast(
    name: &syn::Ident,
    variants: &Punctuated<syn::Variant, Token![,]>,
) -> (Tokens, Tokens) {
    let mut fs: Vec<_> = variants
        .iter()
        .map(|variant| {
            let id = variant.ident.clone();
            let hash = idl_hash(&id.to_string());
            let (ty, idents) = struct_from_ast(&variant.fields);
            Variant {
                ident: id,
                hash,
                ty,
                members: idents,
            }
        })
        .collect();
    let unique: BTreeSet<_> = fs.iter().map(|Variant { hash, .. }| hash).collect();
    assert_eq!(unique.len(), fs.len());
    fs.sort_unstable_by_key(|Variant { hash, .. }| *hash);

    let id = fs.iter().map(|Variant { ident, .. }| ident.to_string());
    let hash = fs.iter().map(|Variant { hash, .. }| hash);
    let ty = fs.iter().map(|Variant { ty, .. }| ty);
    let ty_gen = quote! {
        dfx_info::types::Type::Variant(
            vec![
                #(dfx_info::types::Field {
                    id: #id.to_owned(),
                    hash: #hash,
                    ty: #ty }
                ),*
            ]
        )
    };

    let id = fs.iter().map(|Variant { ident, .. }| {
        syn::parse_str::<Tokens>(&format!("{}::{}", name, ident)).unwrap()
    });
    let index = 0..fs.len() as u64;
    let (pattern, members): (Vec<_>, Vec<_>) = fs
        .iter()
        .map(|f| {
            let (pattern, id) = f.to_pattern();
            (
                pattern,
                quote! {
                    #(dfx_info::Compound::serialize_element(&mut ser, #id)?;)*
                },
            )
        })
        .unzip();
    let variant_gen = quote! {
        match *self {
            #(#id #pattern => {
                let mut ser = __serializer.serialize_variant(#index)?;
                #members
            }),*
        };
        Ok(())
    };
    (ty_gen, variant_gen)
}

fn serialize_struct(idents: &[Ident]) -> Tokens {
    let id = idents.iter().map(|ident| ident.to_token());
    quote! {
        let mut ser = __serializer.serialize_struct()?;
        #(dfx_info::Compound::serialize_element(&mut ser, &self.#id)?;)*
        Ok(())
    }
}

fn struct_from_ast(fields: &syn::Fields) -> (Tokens, Vec<Ident>) {
    match *fields {
        syn::Fields::Named(ref fields) => {
            let (fs, idents) = fields_from_ast(&fields.named);
            (quote! { dfx_info::types::Type::Record(#fs) }, idents)
        }
        syn::Fields::Unnamed(ref fields) => {
            let (fs, idents) = fields_from_ast(&fields.unnamed);
            (quote! { dfx_info::types::Type::Record(#fs) }, idents)
        }
        syn::Fields::Unit => (quote! { dfx_info::types::Type::Null }, Vec::new()),
    }
}

#[derive(Clone)]
enum Ident {
    Named(syn::Ident),
    Unnamed(u32),
}
impl Ident {
    fn to_string(&self) -> String {
        match *self {
            Ident::Named(ref ident) => ident.to_string(),
            Ident::Unnamed(ref i) => (*i).to_string(),
        }
    }
    fn to_token(&self) -> Tokens {
        match self {
            Ident::Named(ident) => quote! { #ident },
            Ident::Unnamed(ref i) => syn::parse_str::<Tokens>(&format!("{}", i)).unwrap(),
        }
    }
}

struct Field {
    ident: Ident,
    hash: u32,
    ty: Tokens,
}

fn fields_from_ast(fields: &Punctuated<syn::Field, syn::Token![,]>) -> (Tokens, Vec<Ident>) {
    let mut fs: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let (ident, hash) = match field.ident {
                Some(ref ident) => (Ident::Named(ident.clone()), idl_hash(&ident.to_string())),
                None => (Ident::Unnamed(i as u32), i as u32),
            };
            Field {
                ident,
                hash,
                ty: derive_type(&field.ty),
            }
        })
        .collect();
    let unique: BTreeSet<_> = fs.iter().map(|Field { hash, .. }| hash).collect();
    assert_eq!(unique.len(), fs.len());
    fs.sort_unstable_by_key(|Field { hash, .. }| *hash);

    let id = fs.iter().map(|Field { ident, .. }| ident.to_string());
    let hash = fs.iter().map(|Field { hash, .. }| hash);
    let ty = fs.iter().map(|Field { ty, .. }| ty);
    let ty_gen = quote! {
        vec![
            #(dfx_info::types::Field {
                id: #id.to_owned(),
                hash: #hash,
                ty: #ty }
            ),*
        ]
    };
    let idents: Vec<Ident> = fs.iter().map(|Field { ident, .. }| ident.clone()).collect();
    (ty_gen, idents)
}

fn derive_type(t: &syn::Type) -> Tokens {
    quote! {
        <#t as dfx_info::IDLType>::ty()
    }
}

fn add_trait_bounds(mut generics: Generics) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            let bound = syn::parse_str("::dfx_info::IDLType").unwrap();
            type_param.bounds.push(bound);
        }
    }
    generics
}