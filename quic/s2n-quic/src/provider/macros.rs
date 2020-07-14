macro_rules! impl_provider_utils {
    () => {
        /// Converts a value into a `Provider`
        pub trait TryInto {
            type Provider: Provider;
            type Error;

            fn try_into(self) -> Result<Self::Provider, Self::Error>;
        }

        #[doc(hidden)]
        pub trait With<T: Provider> {
            type Output;

            fn with(self, provider: T) -> Self::Output;
        }

        /// Automatically implement anything that implements `Provider`
        impl<T: Provider> TryInto for T {
            type Error = core::convert::Infallible;
            type Provider = T;

            fn try_into(self) -> Result<Self::Provider, Self::Error> {
                Ok(self)
            }
        }
    };
}

macro_rules! impl_providers_state {
    (
        $(#[$($attr:tt)*])* struct Providers {
            $(
                $(
                    #[doc = $($doc:tt)*]
                )*
                #[default = $default_ty:ty]
                $field:ident: $field_ty:ident
            ),* $(,)?
        }

        $(
            #[doc = $($trait_doc:tt)*]
        )*
        trait $trait:ident {

        }
    ) => {
        #[doc(hidden)]
        $(
            #[$($attr)*]
        )*
        pub struct Providers<$($field_ty,)*> {
            $(
                $(
                    #[doc = $($doc)*]
                )*
                $field: $field_ty,
            )*
        }

        $(
            #[doc = $($trait_doc)*]
        )*
        pub trait $trait {
            $(
                #[doc(hidden)]
                type $field_ty: $field::Provider;
            )*

            #[doc(hidden)]
            fn build(self) -> Providers<$(Self::$field_ty,)*>;

            #[doc(hidden)]
            fn as_ref(&self) -> Providers<$(&Self::$field_ty,)*>;

            #[doc(hidden)]
            fn as_mut(&mut self) -> Providers<$(&mut Self::$field_ty,)*>;
        }

        #[doc(hidden)]
        impl<$($field_ty: $field::Provider,)*> $trait for Providers<$($field_ty,)*> {
            $(
                #[doc(hidden)]
                type $field_ty = $field_ty;
            )*

            #[doc(hidden)]
            fn build(self) -> Providers<$(Self::$field_ty,)*> {
                self
            }

            #[doc(hidden)]
            fn as_ref(&self) -> Providers<$(&Self::$field_ty,)*> {
                Providers {
                    $(
                        $field: &self.$field,
                    )*
                }
            }

            #[doc(hidden)]
            fn as_mut(&mut self) -> Providers<$(&mut Self::$field_ty,)*> {
                Providers {
                    $(
                        $field: &mut self.$field,
                    )*
                }
            }
        }

        /// The recommended providers for the endpoint.
        ///
        /// The implementation details are intentionally hidden and may
        /// change between releases.
        #[derive(Debug, Default)]
        pub struct DefaultProviders {
            providers: Providers<$($default_ty,)*>
        }

        #[doc(hidden)]
        impl $trait for DefaultProviders {
            $(
                #[doc(hidden)]
                type $field_ty = $default_ty;
            )*

            #[doc(hidden)]
            fn build(self) -> Providers<$(Self::$field_ty,)*> {
                self.providers
            }

            #[doc(hidden)]
            fn as_ref(&self) -> Providers<$(&Self::$field_ty,)*> {
                Providers {
                    $(
                        $field: &self.providers.$field,
                    )*
                }
            }

            #[doc(hidden)]
            fn as_mut(&mut self) -> Providers<$(&mut Self::$field_ty,)*> {
                Providers {
                    $(
                        $field: &mut self.providers.$field,
                    )*
                }
            }
        }

        impl_providers_state!(@with, {}, {$($field: $field_ty),*});
    };
    (@with, { $($field:ident: $field_ty:ident),* }, {}) => {
        // done
    };
    (@with, { $($prev:ident: $prev_ty:ident),* }, { $field:ident: $field_ty:ident $(, $rest:ident: $rest_ty:ident)* }) => {
        impl<Provider: ServerProviders, New: $field::Provider> $field::With<New> for Provider {
            type Output = Providers<$(Provider::$prev_ty, )* New $(, Provider::$rest_ty)*>;

            fn with(self, $field: New) -> Self::Output {
                let providers = self.build();
                Providers {
                    $field,
                    $(
                        $prev: providers.$prev,
                    )*
                    $(
                        $rest: providers.$rest,
                    )*
                }
            }
        }

        impl_providers_state!(@with, { $($prev: $prev_ty, )* $field: $field_ty }, {$($rest: $rest_ty),*});
    }
}

macro_rules! impl_provider_method {
    ($(#[$($attr:tt)*])* $name:ident, $field:ident) => {
            $(
                #[$($attr)*]
            )*
            pub fn $name<T>(self, $field: T) -> Result<Builder<impl ServerProviders>, T::Error>
            where
                T: $field::TryInto,
                Providers: $field::With<T::Provider>,
            {
                let $field = $field.try_into()?;
                let providers = $field::With::with(self.0.build(), $field);
                Ok(Builder(providers))
            }

    };
}
