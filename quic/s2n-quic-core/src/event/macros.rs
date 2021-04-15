// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! events {
    ($(
        #[name = $name_str:literal]
        $(#[$attrs:meta])*
        struct $name:ident $(<$lt:lifetime>)? {
            pub meta: Meta,
            $( pub $field_name:ident : $field_type:ty, )*
        }
    )*) => {

        pub mod events {
            //! A set of events that the application can emit.

            use super::*;
            $(
                $(#[$attrs])*
                #[non_exhaustive]
                #[derive(Clone, Debug, Default)]
                pub struct $name $(<$lt>)? {
                    pub meta: Meta,
                    $( pub $field_name: $field_type, )*
                }

                impl $(<$lt>)? Event for $name $(<$lt>)? {
                    const NAME: &'static str = $name_str;
                }

                paste! {
                    impl $(<$lt>)? $name $(<$lt>)? {
                        pub fn builder() -> builder::[<$name MetaBuilder>] $(<$lt>)? {
                            builder::[<$name MetaBuilder>] ($name::default())
                        }
                    }
                }
            )*

            pub mod builder {
                //! Builders to ensure that Events are created uniformly.
                //!
                //! Event builders allow for an ergonomic way to create Events
                //! and allow us to treat Events as immutable once built.
                //!
                //! Each `Event` has two set of builder associated with it, the
                //! EventMetaBuilder and the EventBuilder.
                //!
                //! The EventMetaBilder captures the Meta field associated with all
                //! Events. Its functions return a EventBuilder.
                //!
                //! The EventBuilder allow for chaining and allows for easy modification
                //! of values of each event. The `fn build` can be envoked on each
                //! EventBuilder to return an Event.

                use super::*;
                $(
                    paste! {
                        /// A builder to ensure we specify a Meta for each event.
                        #[derive(Clone, Debug)]
                        pub struct [<$name MetaBuilder>] $(<$lt>)? (
                            pub(super) events::$name $(<$lt>)?
                        );

                        #[allow(dead_code)]
                        impl $(<$lt>)? [<$name MetaBuilder>] $(<$lt>)? {
                            pub fn with_meta(self, meta: Meta) -> [<$name Builder>] $(<$lt>)? {
                                let mut event = self.0;
                                event.meta = meta;
                                [<$name Builder>] (event)
                            }

                            pub fn without_meta(self) -> [<$name Builder>] $(<$lt>)? {
                                [<$name Builder>] (self.0)
                            }
                        }

                        /// A builder to allow for easy customization of event fields and ensure the
                        /// event is built only once.
                        #[derive(Clone, Debug)]
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
                    fn [<on_ $name:snake>](&mut self, event: &events::$name) {
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
                    fn [<on_ $name:snake>](&mut self, event: &events::$name) {
                        self.0.[<on_ $name:snake>](event);
                        self.1.[<on_ $name:snake>](event);
                    }
                );
            )*
        }
    };
}
