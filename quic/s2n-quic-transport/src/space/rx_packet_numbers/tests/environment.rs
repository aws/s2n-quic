use crate::contexts::testing::{MockWriteContext, OutgoingFrameBuffer};
use s2n_quic_core::{endpoint, time::Timestamp, transmission};

#[derive(Clone, Debug)]
pub struct TestEnvironment {
    pub sent_frames: OutgoingFrameBuffer,
    pub current_time: Timestamp,
    pub transmission_constraint: transmission::Constraint,
    pub local_endpoint_type: endpoint::Type,
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
            sent_frames,
            current_time: s2n_quic_platform::time::now(),
            transmission_constraint: transmission::Constraint::None,
            local_endpoint_type: endpoint::Type::Server,
        }
    }

    pub fn context(&mut self) -> MockWriteContext {
        MockWriteContext::new(
            self.current_time,
            &mut self.sent_frames,
            self.transmission_constraint,
            self.local_endpoint_type,
        )
    }
}
