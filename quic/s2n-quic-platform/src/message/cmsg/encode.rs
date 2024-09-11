// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::features;
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

#[derive(Clone, Copy, Debug)]
pub struct Error;

pub trait Encoder {
    /// Encodes the given value as a control message in the cmsg buffer.
    ///
    /// The msghdr.msg_control should be zero-initialized and aligned and contain enough
    /// room for the value to be written.
    fn encode_cmsg<T: Copy>(
        &mut self,
        level: libc::c_int,
        ty: libc::c_int,
        value: T,
    ) -> Result<usize, Error>;

    /// Encodes ECN markings into the cmsg encoder
    #[inline]
    fn encode_ecn(
        &mut self,
        ecn: ExplicitCongestionNotification,
        remote_address: &SocketAddress,
    ) -> Result<usize, Error> {
        // no need to encode for the default case
        if ecn == ExplicitCongestionNotification::NotEct {
            return Ok(0);
        }

        // the remote address needs to be unmapped in order to set the appropriate cmsg
        match remote_address.unmap() {
            SocketAddress::IpV4(_) => {
                if let (Some(level), Some(ty)) = (features::tos_v4::LEVEL, features::tos_v4::TYPE) {
                    return self.encode_cmsg(level, ty, ecn as u8 as features::tos_v4::Cmsg);
                }
            }
            SocketAddress::IpV6(_) => {
                if let (Some(level), Some(ty)) = (features::tos_v6::LEVEL, features::tos_v6::TYPE) {
                    return self.encode_cmsg(level, ty, ecn as u8 as features::tos_v6::Cmsg);
                }
            }
        }

        Ok(0)
    }

    /// Encodes GSO segment_size into the cmsg encoder
    #[inline]
    fn encode_gso(&mut self, segment_size: u16) -> Result<usize, Error> {
        if let (Some(level), Some(ty)) = (features::gso::LEVEL, features::gso::TYPE) {
            let segment_size = segment_size as features::gso::Cmsg;
            self.encode_cmsg(level, ty, segment_size)
        } else {
            panic!("platform does not support GSO");
        }
    }

    #[inline]
    fn encode_local_address(&mut self, address: &SocketAddress) -> Result<usize, Error> {
        use s2n_quic_core::inet::Unspecified;

        match address {
            SocketAddress::IpV4(addr) => {
                use features::pktinfo_v4 as pktinfo;
                if let (Some(level), Some(ty)) = (pktinfo::LEVEL, pktinfo::TYPE) {
                    let ip = addr.ip();

                    if ip.is_unspecified() {
                        return Ok(0);
                    }

                    let value = pktinfo::encode(ip, None);
                    return self.encode_cmsg(level, ty, value);
                }
            }
            SocketAddress::IpV6(addr) => {
                use features::pktinfo_v6 as pktinfo;
                if let (Some(level), Some(ty)) = (pktinfo::LEVEL, pktinfo::TYPE) {
                    let ip = addr.ip();

                    if ip.is_unspecified() {
                        return Ok(0);
                    }

                    let value = pktinfo::encode(ip, None);
                    return self.encode_cmsg(level, ty, value);
                }
            }
        }

        Ok(0)
    }
}
