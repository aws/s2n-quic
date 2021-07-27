// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! events {
    ($(
        #[name = $name_str:literal]
        $(#[$attrs:meta])*
        struct $name:ident $(<$lt:lifetime>)? {
            $( pub $field_name:ident : $field_type:ty, )*
        }
    )*) => {

        pub mod events {
            //! A set of events that the application can handle

            use super::*;

            $(
                $(#[$attrs])*
                #[non_exhaustive]
                #[derive(Clone, Debug)]
                pub struct $name $(<$lt>)? {
                    $( pub $field_name: $field_type, )*
                }

                impl $(<$lt>)? Event for $name $(<$lt>)? {
                    const NAME: &'static str = $name_str;
                }
            )*
        }

        mod event_builders {

            use super::*;

            $(
                // Builders are an implementation detail and allow us to create
                // `non_exhaustive` Events outside this crate.
                #[derive(Clone, Debug)]
                pub struct $name $(<$lt>)? {
                    $( pub $field_name: $field_type, )*
                }

                #[doc(hidden)]
                impl $(<$lt>)? From<$name $(<$lt>)?> for events::$name $(<$lt>)? {
                    fn from(builder: $name $(<$lt>)?) -> Self {
                        Self {
                            $(
                                $field_name: builder.$field_name,
                            )*
                        }
                    }
                }
            )*
        }

        /// An interface for handling QUIC events.
        ///
        /// This trait exposes a function for each type of event. The default
        /// implementation simply ignores the event, but can be overridden to
        /// consume the event.
        ///
        /// Applications can provide a custom implementation of `Subscriber` to perform
        /// logging, metrics recording, etc.
        pub trait Subscriber: 'static {
            $(
                paste!(
                    $(#[$attrs])*
                    #[inline(always)]
                    fn [<on_ $name:snake>](&mut self, meta: &common::Meta, event: &events::$name) {
                        let _ = meta;
                        let _ = event;
                    }
                );
            )*

            #[inline(always)]
            fn on_event<E: Event>(&mut self, meta: &common::Meta, event: &E) {
                let _ = meta;
                let _ = event;
            }
        }

        impl<A, B> Subscriber for (A, B)
            where A: Subscriber,
                  B: Subscriber,
        {
            $(
                paste!(
                    #[inline(always)]
                    fn [<on_ $name:snake>](&mut self, meta: &common::Meta, event: &events::$name) {
                        self.0.[<on_ $name:snake>](meta, event);
                        self.1.[<on_ $name:snake>](meta, event);
                    }
                );
            )*

            #[inline(always)]
            fn on_event<E: Event>(&mut self, meta: &common::Meta, event: &E) {
                self.0.on_event(meta, event);
                self.1.on_event(meta, event);
            }
        }

        pub trait Publisher {
            $(
                paste!(
                    $(#[$attrs])*
                    fn [<on_ $name:snake>](&mut self, event: builders::$name);
                );
            )*

            fn quic_version(&self) -> Option<u32>;
        }

        pub struct PublisherSubscriber<'a, Sub: Subscriber> {
            meta: common::Meta,
            /// The QUIC protocol version which is used for this particular connection
            quic_version: Option<u32>,
            subscriber: &'a mut Sub,
        }

        impl<'a, Sub: Subscriber> PublisherSubscriber<'a, Sub> {
            pub fn new(meta: builders::Meta, quic_version: Option<u32>, subscriber: &'a mut Sub) -> PublisherSubscriber<'a, Sub> {
                PublisherSubscriber {
                    meta: meta.into(),
                    quic_version,
                    subscriber
                }
            }
        }

        impl<'a, Sub: Subscriber> core::fmt::Debug for PublisherSubscriber<'a, Sub> {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                f.debug_struct("PublisherSubscriber")
                    .field("meta", &self.meta)
                    .finish()
            }
        }

        impl<'a, Sub: Subscriber> Publisher for PublisherSubscriber<'a, Sub> {
            $(
                paste!(
                    $(#[$attrs])*
                    #[inline(always)]
                    fn [<on_ $name:snake>](&mut self, event: builders::$name) {
                        let event = event.into();
                        self.subscriber.[<on_ $name:snake>](&self.meta, &event);
                        self.subscriber.on_event(&self.meta, &event);
                    }
                );
            )*

            fn quic_version(&self) -> Option<u32> {
                self.quic_version
            }
        }

        #[cfg(any(test, feature = "testing"))]
        pub mod testing {
            use super::*;

            pub struct Subscriber;
            impl super::Subscriber for Subscriber{}

            pub struct Publisher;
            impl super::Publisher for Publisher {
                $(
                    super::paste!(
                        $(#[$attrs])*
                        fn [<on_ $name:snake>](&mut self, event: builders::$name) {
                            let event: events::$name = event.into();
                            std::eprintln!("{:?}", event);
                        }
                    );
                )*

                fn quic_version(&self) -> Option<u32> {
                    Some(0)
                }
            }
        }
    };
}

macro_rules! common {
    ($(
        $(#[$attrs:meta])*
        struct $name:ident $(<$lt:lifetime>)? {
            $( pub $field_name:ident : $field_type:ty, )*
        }
    )*
    $(
        $(#[$enum_attrs:meta])*
        enum $enum_name:ident {
            $(
                $(#[$enum_attr:meta])? $enum_fields: ident,
            )*
        }
    )*
    ) => {
        pub mod common {
            //! Common fields that are common to multiple events. Some of these fields exits to
            //! maintain compatibility with the qlog spec.

            use super::*;

            $(
                $(#[$attrs])*
                #[non_exhaustive]
                #[derive(Clone, Debug)]
                pub struct $name $(<$lt>)? {
                    $( pub $field_name : $field_type, )*
                }
            )*

            $(
                $(#[$enum_attrs])*
                #[non_exhaustive]
                #[derive(Copy, Clone, Debug)]
                pub enum $enum_name {
                    $(
                        $(#[$enum_attr])? $enum_fields,
                    )*
                }
            )*
        }

        mod common_builders {

            use super::*;

            $(
                // Builders are an implementation detail and allow us to create
                // `non_exhaustive` Events outside this crate.
                #[derive(Clone, Debug)]
                pub struct $name $(<$lt>)? {
                    $( pub $field_name : $field_type, )*
                }

                #[doc(hidden)]
                impl $(<$lt>)? From<$name $(<$lt>)?> for common::$name $(<$lt>)? {
                    fn from(builder: $name $(<$lt>)?) -> Self {
                        Self {
                            $(
                                $field_name: builder.$field_name,
                            )*
                        }
                    }
                }
            )*
        }
    };
}
