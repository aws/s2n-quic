// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! events {
    ($(
        #[name = $name_str:literal]
        $(#[$attrs:meta])*
        struct $name:ident $(<$lt:lifetime>)? {
            $($fields:tt)*
        }
    )*) => {
        pub mod events {
            use super::*;

            $(
                $(#[$attrs])*
                #[non_exhaustive]
                #[derive(Clone, Debug)]
                pub struct $name $(<$lt>)? {
                    $($fields)*
                }

                impl $(<$lt>)? Event for $name $(<$lt>)? {
                    const NAME: &'static str = $name_str;
                }
            )*
        }

        /// We can implement the Subscriber trait to customize events we
        /// want to emit from the library.
        ///
        /// Since the default implementation is a noop, the rust compiler
        /// is able to optimize away any allocations and code execution. This
        /// results in zero-cost for any event we are not interested in consuming.
        pub trait Subscriber {
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
