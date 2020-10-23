use crate::contexts::testing::{MockConnectionContext, MockWriteContext, OutgoingFrameBuffer};
use s2n_quic_core::{endpoint::EndpointType, time::Timestamp, transmission};

#[derive(Clone, Debug)]
pub struct TestEnvironment {
    pub connection_context: MockConnectionContext,
    pub sent_frames: OutgoingFrameBuffer,
    pub current_time: Timestamp,
    pub transmission_constraint: transmission::Constraint,
}

impl Default for TestEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

impl TestEnvironment {
    pub fn new() -> Self {
        let mut sent_frames = OutgoingFrameBuffer::new();

        sent_frames.set_max_packet_size(Some(1200));

        Self {
            connection_context: MockConnectionContext::new(EndpointType::Server),
            sent_frames,
            current_time: s2n_quic_platform::time::now(),
        }
    }

    pub fn context(&mut self) -> MockWriteContext {
        MockWriteContext::new(
            &self.connection_context,
            self.current_time,
            &mut self.sent_frames,
            self.transmission_constraint,
        )
    }
}
