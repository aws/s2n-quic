// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;

pub mod io;
#[cfg(s2n_quic_platform_socket_mmsg)]
pub mod mmsg;
#[cfg(s2n_quic_platform_socket_msg)]
pub mod msg;
pub mod ring;
pub mod std;

cfg_if! {
    if #[cfg(s2n_quic_platform_socket_mmsg)] {
        pub use mmsg as default;
    } else if #[cfg(s2n_quic_platform_socket_msg)] {
        pub use msg as default;
    } else {
        pub use self::std as default;
    }
}
