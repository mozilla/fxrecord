// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/

extern crate proc_macro;

use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream};
use syn::{parse_quote, Attribute, Ident, ItemEnum, ItemStruct, Meta, Token};

/// Generate message types and implementations.
///
/// Example:
/// ```
/// # extern crate libfxrecord;
/// # #[macro_use] extern crate libfxrecord_macros;
/// #
/// # use std::convert::TryFrom;
/// #
/// # use derive_more::Display;
/// # use libfxrecord::net::{KindMismatch, Message, MessageContent};
/// # use serde::{Deserialize, Serialize};
/// #
/// message_type! {
///     # #[derive(Eq, PartialEq)]
///     MessageType,
///     MessageKind;
///
///     # #[derive(Clone, Copy, Eq, PartialEq)]
///     pub struct StructVariant {
///         pub field: i32,
///     }
///
///     # #[derive(Clone, Copy, Eq, PartialEq)]
///     pub enum EnumVariant {
///         Foo(char),
///         Bar(bool),
///     }
/// }
/// # let s = StructVariant { field: 123 };
/// # let e = EnumVariant::Foo('A');
/// # assert_eq!(StructVariant::kind(), MessageKind::StructVariant);
/// # assert_eq!(EnumVariant::kind(), MessageKind::EnumVariant);
/// # assert_eq!(MessageType::StructVariant(s).kind(), MessageKind::StructVariant);
/// # assert_eq!(MessageType::EnumVariant(e).kind(), MessageKind::EnumVariant);
/// #
/// # assert_eq!(MessageType::from(s), MessageType::StructVariant(s));
/// # assert_eq!(MessageType::from(e), MessageType::EnumVariant(e));
/// #
/// # assert_eq!(StructVariant::try_from(MessageType::StructVariant(s)).unwrap(), s);
/// # assert_eq!(EnumVariant::try_from(MessageType::EnumVariant(e)).unwrap(), e);
/// #
/// # assert!(StructVariant::try_from(MessageType::EnumVariant(e)).is_err());
/// # assert!(EnumVariant::try_from(MessageType::StructVariant(s)).is_err());
/// ```
///
/// This macro generates several items:
///
/// 1. The message type. This is a wrapper enum that contains all message
///    variants. It is the type that is serialized/deserialized by the
///    [`Proto`][Proto]. It will also implement the [`Message`][Message] trait so
///    that its variants can be differentiated by the message kind type.
///
///    In the above example, this is the `MessageType`. The following type would be
///    generated:
///    ```
///    # type StructVariant = (); // Omitted.
///    # type EnumVariant = (); // Omittd.
///    pub enum MessageType {
///        StructVariant(StructVariant),
///        EnumVariant(EnumVariant),
///    }
///    ```
///
/// 2. A message kind type. This is an enum with one variant for each kind of
///    message. This kind is used to differentiate between various messages that are
///    part of the same message enum.
///
///    In the above example, this is `MessageKind`. For that example, the following
///    type would be generated:
///    ```
///    pub enum MessageKind {
///        StructVariant,
///        EnumVariant,
///    }
///
/// 3. Message content types for each message.
///
///    In the above example, these are `StructVariant` and `EnumVariant`. They are
///    emitted verbatim as they are in the macro input. For example, the following
///    types would be emitted for the above example:
///
///    ```
///     pub struct StructVariant {
///         pub field: i32,
///     }
///
///     pub enum EnumVariant {
///         Foo(f32),
///         Bar(String),
///     }
///     ```
///
/// 4. Implementations of [`MessageContent`][MessageContent] for each variant,
///    tying it to its `MessageType` and `MessageKind`.
///
/// 5. Conversion traits between the variants and the message type.
///
///    For each variant, the following impls are emitted:
///    * [`From<Variant> for MessageType`][From]
///    * [`TryFrom<MessageType> for Variant`][TryFrom].
///
/// [Proto]: ../libfxrecord/net/proto/struct.Proto.html
/// [Message]: ../libfxrecord/net/message/trait.Message.html
/// [MessageContent]: ../libfxrecord/net/message/trait.MessageContent.html
/// [From]: https://doc.rust-lang.org/std/convert/trait.From.html
/// [TryFrom]: https://doc.rust-lang.org/std/convert/trait.TryFrom.html
#[proc_macro]
pub fn message_type(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let decl = match syn::parse::<MessageDecl>(input) {
        Ok(decl) => decl,
        Err(e) => return e.to_compile_error().into(),
    };

    let msg_kind = generate_message_kind_type(&decl);
    let msg_ty = generate_message_type(&decl);
    let variant = &decl.variants;
    let impls = generate_impls(&decl);

    let tokens = quote! {
        #msg_kind
        #msg_ty
        #(
            #[derive(Debug, Deserialize, Serialize)]
            #variant
        )*
        #impls
    };

    tokens.into()
}

/// The body of the `message_type!{}` macro.
struct MessageDecl {
    /// The type declaration for the message enumeration.
    msg_ty: TyDecl,
    _comma: Token![,],
    /// The type declaration for the message kind enumeration.
    kind_ty: TyDecl,
    _semi: Token![;],
    /// The message variants.
    variants: Vec<VariantDecl>,
}

impl Parse for MessageDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(MessageDecl {
            msg_ty: input.parse()?,
            _comma: input.parse()?,
            kind_ty: input.parse()?,
            _semi: input.parse()?,
            variants: {
                let mut variants = vec![];
                loop {
                    variants.push(input.parse()?);
                    if input.is_empty() {
                        break variants;
                    }
                }
            },
        })
    }
}

/// A type declaration.
struct TyDecl {
    /// Attributes (e.g., doc comments) for the type.
    attrs: Vec<Attribute>,
    /// The name of the type to be declared.
    ident: Ident,
}

impl Parse for TyDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(TyDecl {
            attrs: Attribute::parse_outer(input)?,
            ident: input.parse()?,
        })
    }
}

/// A message variant.
struct VariantDecl {
    /// The outer attributes of the variant.
    ///
    /// These are likely entirely doc comments.
    attrs: Vec<Attribute>,

    /// The struct or enum item representing the variant.
    inner: VariantDeclInner,
}

impl Parse for VariantDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(VariantDecl {
            attrs: Attribute::parse_outer(input)?,
            inner: input.parse()?,
        })
    }
}

impl ToTokens for VariantDecl {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for attr in &self.attrs {
            attr.to_tokens(tokens);
        }
        self.inner.to_tokens(tokens);
    }
}

/// A message variant, represented as either an `enum` or a `struct`.
enum VariantDeclInner {
    Struct(ItemStruct),
    Enum(ItemEnum),
}

impl ToTokens for VariantDeclInner {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Struct(ref s) => s.to_tokens(tokens),
            Self::Enum(ref e) => e.to_tokens(tokens),
        }
    }
}

impl Parse for VariantDeclInner {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let vis: Token![pub] = input.parse()?;

        let lookahead = input.lookahead1();
        if lookahead.peek(Token![struct]) {
            input.parse().map(|mut s: ItemStruct| {
                s.vis = parse_quote! { #vis };
                VariantDeclInner::Struct(s)
            })
        } else if lookahead.peek(Token![enum]) {
            input.parse().map(|mut e: ItemEnum| {
                e.vis = parse_quote! { #vis };
                VariantDeclInner::Enum(e)
            })
        } else {
            Err(lookahead.error())
        }
    }
}

/// Generate the message kind enumeration.
fn generate_message_kind_type(decl: &MessageDecl) -> proc_macro2::TokenStream {
    let kind_ty = &decl.kind_ty.ident;
    let kind_ty_attr = &decl.kind_ty.attrs;
    let variant = decl.variants.iter().map(|variant| match variant.inner {
        VariantDeclInner::Struct(ref s) => &s.ident,
        VariantDeclInner::Enum(ref e) => &e.ident,
    });

    quote! {
        #(#kind_ty_attr)*
        #[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
        pub enum #kind_ty {
            #(#variant,)*
        }
    }
}

/// Generate the message enumeration.
fn generate_message_type(decl: &MessageDecl) -> proc_macro2::TokenStream {
    let kind_ty = &decl.kind_ty.ident;
    let msg_ty = &decl.msg_ty.ident;
    let msg_ty_attr = &decl.msg_ty.attrs;
    let variant = decl.variants.iter().map(|variant| match variant.inner {
        VariantDeclInner::Struct(ref s) => &s.ident,
        VariantDeclInner::Enum(ref e) => &e.ident,
    });

    let msg_ty_variant = decl.variants.iter().map(|variant| {
        let ident = match variant.inner {
            VariantDeclInner::Struct(ref s) => &s.ident,
            VariantDeclInner::Enum(ref e) => &e.ident,
        };

        let doc = variant
            .attrs
            .iter()
            .filter(|attr| match attr.parse_meta().ok() {
                Some(Meta::NameValue(ref kv)) => kv.path.is_ident("doc"),
                _ => false,
            });

        quote! {
            #(#doc)*
            #ident(#ident),
        }
    });

    quote! {
        #(#msg_ty_attr)*
        #[derive(Debug, Deserialize, Serialize)]
        pub enum #msg_ty {
            #(#msg_ty_variant)*
        }

        impl Message<'_> for #msg_ty {
            type Kind = #kind_ty;

            fn kind(&self) -> Self::Kind {
                match self {
                    #(Self::#variant(..) => #kind_ty::#variant,)*
                }
            }
        }
    }
}

/// Generate `From`, `TryFrom`, and `MessageContent` impls for the variants.
fn generate_impls(decl: &MessageDecl) -> proc_macro2::TokenStream {
    let msg_ty = &decl.msg_ty.ident;
    let kind_ty = &decl.kind_ty.ident;

    let variant = decl.variants.iter().map(|variant| match variant.inner {
        VariantDeclInner::Struct(ref s) => &s.ident,
        VariantDeclInner::Enum(ref e) => &e.ident,
    });

    quote! {
        #(
            impl ::std::convert::From<#variant> for #msg_ty {
                fn from(m: #variant) -> Self {
                    #msg_ty::#variant(m)
                }
            }

            impl ::std::convert::TryFrom<#msg_ty> for #variant {
                type Error = KindMismatch<#kind_ty>;

                fn try_from(msg: #msg_ty) -> Result<Self, Self::Error> {
                    #[allow(irrefutable_let_patterns)]
                    if let #msg_ty::#variant(inner) = msg {
                        Ok(inner)
                    } else {
                        Err(KindMismatch {
                            expected: #kind_ty::#variant,
                            actual: msg.kind(),
                        })
                    }
                }
            }

            impl MessageContent<'_, #msg_ty, #kind_ty> for #variant {
                fn kind() -> #kind_ty {
                    #kind_ty::#variant
                }
            }
        )*
    }
}
