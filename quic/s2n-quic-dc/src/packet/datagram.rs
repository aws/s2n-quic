use super::{tag::Common, HeaderLen, PacketNumber, PayloadLen};
use core::fmt;
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};

pub mod decoder;
pub mod encoder;

#[derive(Clone, Copy, PartialEq, Eq, AsBytes, FromBytes, FromZeroes, Unaligned)]
#[repr(C)]
pub struct Tag(Common);

impl_tag_codec!(Tag);

impl Default for Tag {
    #[inline]
    fn default() -> Self {
        Self(Common(0b0100_0000))
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("datagram::Tag")
            .field("ack_eliciting", &self.ack_eliciting())
            .field("is_connected", &self.is_connected())
            .field("has_length", &self.has_length())
            .field("has_application_header", &self.has_application_header())
            .finish()
    }
}

impl Tag {
    const ACK_ELICITING_MASK: u8 = 0b1000;
    const IS_CONNECTED_MASK: u8 = 0b0100;
    const HAS_LENGTH_MASK: u8 = 0b0010;
    const HAS_APPLICATION_HEADER_MASK: u8 = 0b0001;

    pub const MIN: u8 = 0b0100_0000;
    pub const MAX: u8 = 0b0100_1111;

    #[inline]
    pub fn set_ack_eliciting(&mut self, enabled: bool) {
        self.0.set(Self::ACK_ELICITING_MASK, enabled)
    }

    #[inline]
    pub fn ack_eliciting(&self) -> bool {
        self.0.get(Self::ACK_ELICITING_MASK)
    }

    #[inline]
    pub fn set_is_connected(&mut self, enabled: bool) {
        self.0.set(Self::IS_CONNECTED_MASK, enabled)
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.0.get(Self::IS_CONNECTED_MASK)
    }

    #[inline]
    pub fn set_has_length(&mut self, enabled: bool) {
        self.0.set(Self::HAS_LENGTH_MASK, enabled)
    }

    #[inline]
    pub fn has_length(&self) -> bool {
        self.0.get(Self::HAS_LENGTH_MASK)
    }

    #[inline]
    pub fn set_has_application_header(&mut self, enabled: bool) {
        self.0.set(Self::HAS_APPLICATION_HEADER_MASK, enabled)
    }

    #[inline]
    pub fn has_application_header(&self) -> bool {
        self.0.get(Self::HAS_APPLICATION_HEADER_MASK)
    }

    #[inline]
    fn validate(&self) -> Result<(), s2n_codec::DecoderError> {
        let range = Self::MIN..=Self::MAX;
        s2n_codec::decoder_invariant!(range.contains(&(self.0).0), "invalid datagram bit pattern");
        Ok(())
    }
}
