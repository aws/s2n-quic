use core::{
    fmt,
    ops::{Deref, DerefMut},
};
use s2n_codec::zerocopy_value_codec;
use s2n_quic_core::varint::VarInt;
use zerocopy::{AsBytes, FromBytes, Unaligned};

#[derive(Clone, Copy, PartialEq, Eq, AsBytes, FromBytes, Unaligned)]
#[repr(C)]
pub struct Tag(u8);

zerocopy_value_codec!(Tag);

impl Default for Tag {
    #[inline]
    fn default() -> Self {
        Self(0b0110_0000)
    }
}

/*
impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("datagram::Tag")
            .field("mode", &self.mode())
            .finish()
    }
}

impl Tag {
    #[inline]
    pub fn mode(&self) -> Mode {

    }
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Early,
    Authenticated,
    Stateless,
}
*/
