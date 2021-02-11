use s2n_codec::{EncoderBuffer, EncoderValue};

/// A buffer used by an endpoint to encode transport parameters
///
/// We do a single allocation at the endpoint since s2n-tls will just
/// copy the buffer anyway
#[derive(Debug, Default)]
pub struct Params {
    buffer: Vec<u8>,
}

impl Params {
    pub fn with<P, F, R>(&mut self, params: &P, f: F) -> R
    where
        P: EncoderValue,
        F: FnOnce(&[u8]) -> R,
    {
        let len = params.encoding_size();
        self.buffer.resize(len, 0);
        params.encode(&mut EncoderBuffer::new(&mut self.buffer));
        f(&self.buffer)
    }
}
