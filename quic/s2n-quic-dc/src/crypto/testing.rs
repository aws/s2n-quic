use super::IntoNonce;
use crate::credentials::Credentials;
use s2n_quic_core::assume;

#[derive(Clone, Debug)]
pub struct Key {
    credentials: Credentials,
    tag_len: usize,
}

impl Key {
    #[inline]
    pub fn new(credentials: Credentials) -> Self {
        Self {
            credentials,
            tag_len: 16,
        }
    }
}

impl super::encrypt::Key for Key {
    #[inline]
    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.tag_len
    }

    #[inline]
    fn encrypt<N: IntoNonce>(
        &self,
        _nonce: N,
        _header: &[u8],
        extra_payload: Option<&[u8]>,
        payload_and_tag: &mut [u8],
    ) -> Result<(), super::encrypt::Error> {
        if let Some(extra_payload) = extra_payload {
            let offset = payload_and_tag.len() - self.tag_len() - extra_payload.len();
            let dest = &mut payload_and_tag[offset..];
            unsafe {
                assume!(dest.len() == extra_payload.len() + self.tag_len);
            }
            let (dest, tag) = dest.split_at_mut(extra_payload.len());
            dest.copy_from_slice(extra_payload);
            tag.fill(0);
        }

        Ok(())
    }

    #[inline]
    fn retransmission_tag(
        &self,
        _original_packet_number: u64,
        _retransmission_packet_number: u64,
        _tag_out: &mut [u8],
    ) {
        // no-op
    }
}

impl super::decrypt::Key for Key {
    #[inline]
    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.tag_len
    }

    #[inline]
    fn decrypt<N: IntoNonce>(
        &mut self,
        _nonce: N,
        _header: &[u8],
        payload_in: &[u8],
        _tag: &[u8],
        payload_out: &mut bytes::buf::UninitSlice,
    ) -> Result<(), super::decrypt::Error> {
        payload_out.copy_from_slice(payload_in);
        Ok(())
    }

    #[inline]
    fn decrypt_in_place<N: IntoNonce>(
        &mut self,
        _nonce: N,
        _header: &[u8],
        _payload_and_tag: &mut [u8],
    ) -> Result<(), super::decrypt::Error> {
        Ok(())
    }

    #[inline]
    fn retransmission_tag(
        &mut self,
        _original_packet_number: u64,
        _retransmission_packet_number: u64,
        _tag_out: &mut [u8],
    ) {
        // no-op
    }
}
