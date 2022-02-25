// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use crate::config;
pub use netbench::{
    scenario::{builder, Scenario},
    units::*,
    Result,
};

#[macro_export]
macro_rules! config {
    ({$($(#[doc = $doc:literal])* let $name:ident: $ty:ty = $default:expr;)*}) => {
        #[derive(Debug)]
        pub struct Config {
            $(
                $(#[doc = $doc])*
                $name: $ty,
            )*
        }

        impl Config {
            pub fn register(registry: &mut $crate::config::Registry) {
                $(
                    {
                        let name = concat!(module_path!(), ".", stringify!($name)).split("::").last().unwrap();
                        static DOCS: &[&str] = &[$($doc),*];
                        let default: $ty = $default;
                        registry.define(name, DOCS, &default)
                    }
                )*
            }

            pub fn new(overrides: &mut $crate::config::Overrides) -> Config {
                Config {
                    $(
                        $name: {
                            let name = concat!(module_path!(), ".", stringify!($name)).split("::").last().unwrap();
                            overrides.resolve(name, $default)
                        },
                    )*
                }
            }
        }
    };
}
