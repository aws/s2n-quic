// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0


#[non_exhaustive]
#[derive(Clone, Debug, Default)]
struct Foo<'a> {
    pub server_alpns: &'a [&'a [u8]],
    pub meta: super::Meta,
}

#[allow(dead_code)]
impl<'a> Foo<'a> {
    pub fn builder() -> FooMetaBuilder<'a> {
        FooMetaBuilder(Foo::default())
    }
}

struct FooMetaBuilder<'a>(Foo<'a>);

#[allow(dead_code)]
impl<'a> FooMetaBuilder<'a> {
    pub fn with_meta(self, meta: super::Meta) -> FooBuilder<'a> {
        let mut event = self.0;
        event.meta = meta;
        FooBuilder(event)
    }

    pub fn without_meta(self) -> FooBuilder<'a> {
        let event = self.0;
        FooBuilder(event)
    }
}

struct FooBuilder<'a>(Foo<'a>);

#[allow(dead_code)]
impl<'a> FooBuilder<'a> {
    fn with_server_alpns(&mut self) {
    }

    fn build(self) -> Foo<'a> {
        self.0
    }
}

fn _bla() {
    let _e = Foo::builder().without_meta().build();
}

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
            use super::*;

            $(
                $(#[$attrs])*
                #[non_exhaustive]
                #[derive(Clone, Debug, Default)]
                pub struct $name $(<$lt>)? {
                    pub meta: super::Meta,
                    $( pub $field_name: $field_type, )*
                }

                impl $(<$lt>)? Event for $name $(<$lt>)? {
                    const NAME: &'static str = $name_str;
                }

                paste! {
                    impl $(<$lt>)? $name $(<$lt>)? {
                        pub fn builder() -> [<$name Builder>] $(<$lt>)? {
                            [<$name Builder>] ($name::default())
                        }

                        $(
                            pub fn [<with_ $field_name>](&mut self, $field_name: $field_type) {
                                self.$field_name = $field_name;
                            }
                        )*
                    }

                    #[derive(Clone, Debug)]
                    pub struct [<$name MetaBuilder>] $(<$lt>)? (
                        $name $(<$lt>)?
                    );

                    #[allow(dead_code)]
                    impl $(<$lt>)? [<$name MetaBuilder>] $(<$lt>)? {
                        pub fn with_meta(self, meta: super::Meta) -> [<$name Builder>] $(<$lt>)? {
                            let mut event = self.0;
                            event.meta = meta;
                            [<$name Builder>] (event)
                        }

                        pub fn without_meta(self) -> [<$name Builder>] $(<$lt>)? {
                            [<$name Builder>] (self.0)
                        }
                    }

                    #[derive(Clone, Debug)]
                    pub struct [<$name Builder>] $(<$lt>)? (
                        $name $(<$lt>)?
                    );

                    #[allow(dead_code)]
                    impl $(<$lt>)? [<$name Builder>] $(<$lt>)? {
                        fn build(self) -> $name $(<$lt>)? {
                            self.0
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
