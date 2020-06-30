//! Utility functions for reading and writing `msgname` fields in a `msghdr`

use core::mem::size_of;
use libc::{msghdr, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6};
use s2n_quic_core::inet::{
    IPv4Address, IPv6Address, SocketAddress, SocketAddressV4, SocketAddressV6,
};

const IN_SIZE: u32 = size_of::<sockaddr_in>() as _;
const IN6_SIZE: u32 = size_of::<sockaddr_in6>() as _;

/// Reads the `SocketAddress` for the given message
pub fn get_msgname(msghdr: &msghdr) -> Option<SocketAddress> {
    match msghdr.msg_namelen {
        IN_SIZE => {
            let sockaddr: &sockaddr_in = unsafe { ptr_as_ref(msghdr.msg_name) };
            let port = sockaddr.sin_port.to_be();
            let addr: IPv4Address = sockaddr.sin_addr.s_addr.to_ne_bytes().into();
            Some(SocketAddressV4::new(addr, port).into())
        }
        IN6_SIZE => {
            let sockaddr: &sockaddr_in6 = unsafe { ptr_as_ref(msghdr.msg_name) };
            let port = sockaddr.sin6_port.to_be();
            let addr: IPv6Address = sockaddr.sin6_addr.s6_addr.into();
            Some(SocketAddressV6::new(addr, port).into())
        }
        _ => None,
    }
}

/// Writes the `SocketAddress` to the given message
pub fn set_msgname(msghdr: &mut msghdr, addr: &SocketAddress) {
    match addr {
        SocketAddress::IPv4(addr) => {
            let sockaddr: &mut sockaddr_in = unsafe { ptr_as_mut(msghdr.msg_name) };
            sockaddr.sin_family = AF_INET as _;
            sockaddr.sin_port = addr.port().to_be();
            sockaddr.sin_addr.s_addr = u32::from_ne_bytes((*addr.ip()).into());
            msghdr.msg_namelen = IN_SIZE;
        }
        SocketAddress::IPv6(addr) => {
            let sockaddr: &mut sockaddr_in6 = unsafe { ptr_as_mut(msghdr.msg_name) };
            sockaddr.sin6_family = AF_INET6 as _;
            sockaddr.sin6_port = addr.port().to_be();
            sockaddr.sin6_addr.s6_addr = (*addr.ip()).into();
            msghdr.msg_namelen = IN6_SIZE;
        }
    }
}

/// Resets the msgname length for the given `msghdr`
///
/// This should be called before returning a message to the queue
pub fn reset_msgname(msghdr: &mut msghdr) {
    msghdr.msg_namelen = IN6_SIZE;
}

#[inline]
unsafe fn ptr_as_ref<'a, T>(ptr: *const libc::c_void) -> &'a T {
    &*(ptr as *const T)
}

#[inline]
unsafe fn ptr_as_mut<'a, T>(ptr: *mut libc::c_void) -> &'a mut T {
    &mut *(ptr as *mut T)
}

#[test]
fn roundtrip_test() {
    use core::mem::zeroed;

    let mut msghdr = unsafe { zeroed::<msghdr>() };

    let mut msgname = unsafe { zeroed::<sockaddr_in6>() };
    msghdr.msg_name = &mut msgname as *mut _ as *mut _;
    msghdr.msg_namelen = IN6_SIZE;

    let tests = ["192.168.1.2:1337", "[2001:db8:85a3::8a2e:370:7334]:1337"];

    for addr in tests.iter() {
        let addr: std::net::SocketAddr = addr.parse().unwrap();
        let addr: SocketAddress = addr.into();

        set_msgname(&mut msghdr, &addr);
        assert_eq!(get_msgname(&msghdr), Some(addr));

        reset_msgname(&mut msghdr);
    }
}
