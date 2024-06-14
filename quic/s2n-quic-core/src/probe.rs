// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[doc(hidden)]
#[macro_export]
macro_rules! __probe_define__ {
    (extern "probe" {
        $(
            $(
                #[doc = $doc:tt]
            )*
            #[link_name = $link_name:ident]
            $vis:vis fn $fun:ident($($arg:ident: $arg_t:ty),* $(,)?);
        )*
    }) => {
        $(
            $(
                #[doc = $doc]
            )*
            #[inline(always)]
            $vis fn $fun($($arg: $arg_t),*) {
                $crate::probe::__trace!(
                    name: stringify!($fun),
                    target: concat!(module_path!(), "::", stringify!($fun)),
                    $(
                        $arg = ?$arg,
                    )*
                );

                $crate::probe::__usdt!(
                    s2n_quic,
                    $link_name,
                    $(
                        $arg
                    ),*
                );
            }
        )*
    }
}

#[doc(inline)]
pub use __probe_define__ as define;

#[cfg(feature = "probe-tracing")]
#[doc(hidden)]
pub use tracing::trace as __trace;

#[cfg(not(feature = "probe-tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! __trace_impl__ {
    ($($fmt:tt)*) => {};
}

#[cfg(not(feature = "probe-tracing"))]
#[doc(hidden)]
pub use __trace_impl__ as __trace;

#[cfg(feature = "usdt")]
#[doc(hidden)]
pub use probe::probe as __usdt_emit__;

#[cfg(feature = "usdt")]
#[doc(hidden)]
#[macro_export]
macro_rules! __usdt_impl__ {
    ($provider:ident, $name:ident $(, $arg:ident)* $(,)?) => {{
        // define a function with inline(never) to consolidate probes to this single location
        let probe = {
            #[inline(never)]
            || {
                $(
                    let $arg = $crate::probe::Arg::into_usdt($arg);
                )*
                $crate::probe::__usdt_emit__!($provider, $name, $($arg),*);
            }
        };
        probe();
    }}
}

#[cfg(not(feature = "usdt"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __usdt_impl__ {
    ($provider:ident, $name:ident $(, $arg:ident)* $(,)?) => {{
        // make sure all of the args implement Arg
        $(
            let _ = $crate::probe::Arg::into_usdt($arg);
        )*
    }}
}

#[doc(inline)]
pub use __usdt_impl__ as __usdt;

pub trait Arg {
    fn into_usdt(self) -> isize;
}

macro_rules! impl_int_arg {
    ($($ty:ident),* $(,)?) => {
        $(
            impl Arg for $ty {
                #[inline]
                fn into_usdt(self) -> isize {
                    self as _
                }
            }
        )*
    }
}

impl_int_arg!(u8, i8, u16, i16, u32, i32, u64, i64, usize, isize);

impl Arg for bool {
    #[inline]
    fn into_usdt(self) -> isize {
        if self {
            1
        } else {
            0
        }
    }
}

impl Arg for core::time::Duration {
    #[inline]
    fn into_usdt(self) -> isize {
        self.as_nanos() as _
    }
}

impl Arg for crate::time::Timestamp {
    #[inline]
    fn into_usdt(self) -> isize {
        unsafe { self.as_duration().into_usdt() }
    }
}

impl Arg for crate::packet::number::PacketNumber {
    #[inline]
    fn into_usdt(self) -> isize {
        self.as_u64().into_usdt()
    }
}

impl Arg for crate::varint::VarInt {
    #[inline]
    fn into_usdt(self) -> isize {
        self.as_u64().into_usdt()
    }
}

#[cfg(test)]
mod tests {
    crate::probe::define!(
        extern "probe" {
            /// Testing the probe capability
            #[link_name = s2n_quic_core__probe__tests__test123]
            pub fn test123(a: u8, b: core::time::Duration, c: u64);
        }
    );

    #[test]
    fn call_probe_test() {
        test123(123, core::time::Duration::from_secs(123), 123);
    }
}
