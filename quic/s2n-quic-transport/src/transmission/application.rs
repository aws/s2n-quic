use crate::{
    contexts::WriteContext,
    space::HandshakeStatus,
    stream::{AbstractStreamManager, StreamTrait as Stream},
    sync::flag::Ping,
    transmission,
};
use core::ops::RangeInclusive;

pub struct Payload<'a, S: Stream> {
    pub handshake_status: &'a mut HandshakeStatus,
    pub ping: &'a mut Ping,
    pub stream_manager: &'a mut AbstractStreamManager<S>,
}

impl<'a, S: Stream> super::Payload for Payload<'a, S> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        // We need at least 1 byte to write a HANDSHAKE_DONE or PING frame
        (*range.start()).max(1)
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        // send HANDSHAKE_DONE frames first, if needed, to ensure the handshake is confirmed as
        // soon as possible
        let _ = self.handshake_status.on_transmit(context);

        let _ = self.stream_manager.on_transmit(context);

        // send PINGs last, since they might not actually be needed if there's an ack-eliciting
        // frame already present in the payload
        let _ = self.ping.on_transmit(context);
    }
}

impl<'a, S: Stream> transmission::interest::Provider for Payload<'a, S> {
    fn transmission_interest(&self) -> transmission::Interest {
        transmission::Interest::default()
            + self.handshake_status.transmission_interest()
            + self.stream_manager.transmission_interest()
    }
}
