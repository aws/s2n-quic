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

        pub mod builders {
            //! Builders allow for ergonomic and uniform creation of Events.

            use super::*;

            $(
                /// A builder to allow for easy customization of event fields and ensure the
                /// event is built only once.
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
                    fn [<on_ $name:snake>](&mut self, meta: &common::Meta, event: &events::$name) {
                        let _ = meta;
                        let _ = event;
                    }
                );
            )*
        }

        impl<A, B> Subscriber for (A, B)
            where A: Subscriber,
                  B: Subscriber,
        {
            $(
                paste!(
                    fn [<on_ $name:snake>](&mut self, meta: &common::Meta, event: &events::$name) {
                        self.0.[<on_ $name:snake>](meta, event);
                        self.1.[<on_ $name:snake>](meta, event);
                    }
                );
            )*
        }

        pub trait Publisher {
            $(
                paste!(
                    $(#[$attrs])*
                    fn [<on_ $name:snake>](&mut self, event: builders::$name);
                );
            )*
        }

        pub struct PublisherSubscriber<'a, Sub: Subscriber> {
            meta: common::Meta,
            subscriber: &'a mut Sub,
        }

        impl<'a, Sub: Subscriber> PublisherSubscriber<'a, Sub> {
            pub fn new(meta: common::Meta, subscriber: &'a mut Sub) -> PublisherSubscriber<'a, Sub> {
                PublisherSubscriber {
                    meta,
                    subscriber
                }
            }
        }

        impl<'a, Sub: Subscriber> Publisher for PublisherSubscriber<'a, Sub> {
            $(
                paste!(
                    $(#[$attrs])*
                    fn [<on_ $name:snake>](&mut self, event: builders::$name) {
                        self.subscriber.[<on_ $name:snake>](&self.meta, &event.into());
                    }
                );
            )*
        }

        /*
         * TODO fix me
        #[cfg(any(test, feature = "testing"))]
        mod tests {
            $( super::paste! {
                #[test]
                fn [<build_ $name:snake>]() {
                    super::events::$name::builder().build();
                }
            } )*
        }
        */

        #[cfg(any(test, feature = "testing"))]
        pub mod testing {
            use super::*;

            pub struct Subscriber;
            impl super::Subscriber for Subscriber{}

            pub struct Publisher;
            impl super::Publisher for Publisher{
                $(
                    super::paste!(
                        $(#[$attrs])*
                        fn [<on_ $name:snake>](&mut self, event: builders::$name) {
                            let event: events::$name = event.into();
                            std::eprintln!("{:?}", event);
                        }
                    );
                )*
            }
        }
    };
}
