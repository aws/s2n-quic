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
            //! A set of events that the application can emit.
            //!
            //! Event should be created using a `builder`. Each Event has two set of
            //! builder associated with it; the EventMetaBuilder and the EventBuilder.
            //!
            //! The EventMetaBilder captures the Meta field associated with all
            //! Events. Its functions return a EventBuilder.
            //!
            //! The EventBuilder allow for chaining and allows for easy modification
            //! of values of each event. The `fn build` can be envoked on each
            //! EventBuilder to finalze and return an Event.

            use super::{Event, events, PacketHeader};
            use paste::paste;
            $(
                $(#[$attrs])*
                #[non_exhaustive]
                #[derive(Clone, Debug, Default)]
                pub struct $name $(<$lt>)? {
                    $( pub $field_name: $field_type, )*
                }

                impl $(<$lt>)? Event for $name $(<$lt>)? {
                    const NAME: &'static str = $name_str;
                }

                paste! {
                    impl $(<$lt>)? $name $(<$lt>)? {
                        pub fn builder() -> builder::[<$name Builder>] $(<$lt>)? {
                            builder::[<$name Builder>]::default()
                        }
                    }
                }
            )*

            pub mod builder {
                //! Builders allow for ergonomic and uniform creation of Events.

                use super::*;
                $(
                    paste! {
                        /// A builder to allow for easy customization of event fields and ensure the
                        /// event is built only once.
                        #[derive(Clone, Debug, Default)]
                        pub struct [<$name Builder>] $(<$lt>)? (
                            events::$name $(<$lt>)?
                        );

                        #[allow(dead_code)]
                        impl $(<$lt>)? [<$name Builder>] $(<$lt>)? {
                            pub fn build(self) -> events::$name $(<$lt>)? {
                                self.0
                            }

                            $(
                                pub fn [<with_ $field_name>](mut self, $field_name: $field_type) -> Self {
                                    self.0.$field_name = $field_name;
                                    self
                                }
                            )*
                        }
                    }
                )*
            }
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
                    fn [<on_ $name:snake>](&mut self, meta: &Meta, event: &events::$name) {
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
                    fn [<on_ $name:snake>](&mut self, meta: &Meta, event: &events::$name) {
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
                    fn [<on_ $name:snake>](&mut self, event: &events::$name);
                );
            )*
        }

        pub struct PublisherSubscriber<'a, Sub: Subscriber> {
            meta: Meta,
            subscriber: &'a mut Sub,
        }

        impl<'a, Sub: Subscriber> PublisherSubscriber<'a, Sub> {
            pub fn new(meta: Meta, subscriber: &'a mut Sub) -> PublisherSubscriber<'a, Sub> {
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
                    fn [<on_ $name:snake>](&mut self, event: &events::$name) {
                        self.subscriber.[<on_ $name:snake>](&self.meta, event);
                    }
                );
            )*
        }

        #[cfg(any(test, feature = "testing"))]
        mod tests {
            $( super::paste! {
                #[test]
                fn [<build_ $name:snake _with_meta>]() {
                    let meta = super::Meta::default();
                    super::events::$name::builder()
                        .with_meta(meta)
                        .build();
                }

                #[test]
                fn [<build_ $name:snake _default_meta>]() {
                    super::events::$name::builder()
                        .default_meta()
                        .build();
                }
            } )*
        }
    };
}
