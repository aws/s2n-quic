// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bolero::check;
use s2n_quic_core::inet::{SocketAddress, Unspecified};

#[test]
fn address_inverse_pair_test() {
    use core::mem::zeroed;

    let mut msghdr = unsafe { zeroed::<msghdr>() };

    let mut msgname = unsafe { zeroed::<sockaddr_in6>() };
    msghdr.msg_name = &mut msgname as *mut _ as *mut _;
    msghdr.msg_namelen = size_of::<sockaddr_in6>() as _;

    let mut iovec = unsafe { zeroed::<iovec>() };
    msghdr.msg_iov = &mut iovec;

    let mut message = msghdr;

    check!()
        .with_type::<SocketAddress>()
        .cloned()
        .for_each(|addr| {
            unsafe {
                message.reset(0);
            }
            message.set_remote_address(&addr);

            assert_eq!(message.remote_address(), Some(addr));
        });
}

#[test]
fn handle_get_set_test() {
    check!()
        .with_generator((
            gen::<Handle>(),
            1..=crate::features::gso::MaxSegments::MAX.into(),
        ))
        .cloned()
        .for_each(|(handle, segment_size)| {
            use core::mem::zeroed;

            let mut msghdr = unsafe { zeroed::<msghdr>() };

            let mut msgname = unsafe { zeroed::<sockaddr_in6>() };
            msghdr.msg_name = &mut msgname as *mut _ as *mut _;
            msghdr.msg_namelen = size_of::<sockaddr_in6>() as _;

            let mut iovec = unsafe { zeroed::<iovec>() };
            let mut iovec_buf = [0u8; 16];
            iovec.iov_len = iovec_buf.len() as _;
            iovec.iov_base = (&mut iovec_buf[0]) as *mut u8 as _;
            msghdr.msg_iov = &mut iovec;

            let mut cmsg_buf = [0u8; cmsg::MAX_LEN];
            msghdr.msg_controllen = cmsg_buf.len() as _;
            msghdr.msg_control = (&mut cmsg_buf[0]) as *mut u8 as _;

            let mut message = msghdr;

            handle.update_msg_hdr(&mut message);

            if segment_size > 1 {
                message.set_segment_size(segment_size);
            }

            let (header, _cmsg) = message.header().unwrap();

            assert_eq!(header.path.remote_address, handle.remote_address);

            if cfg!(s2n_quic_platform_pktinfo) && !handle.local_address.ip().is_unspecified() {
                assert_eq!(header.path.local_address.ip(), handle.local_address.ip());
            }

            // reset the message and ensure everything is zeroed
            unsafe {
                message.reset(0);
            }

            let (header, _cmsg) = msghdr.header().unwrap();
            assert!(header.path.remote_address.is_unspecified());
        });
}
