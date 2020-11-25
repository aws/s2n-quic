use s2n_quic_core::counter::Counter;

#[derive(Debug, Default)]
pub struct Manager {
    inflight_handshakes: Counter<usize>,
}

impl Manager {
    pub fn new() -> Self {
        Self {
            inflight_handshakes: Counter::new(0usize),
        }
    }

    pub fn on_handshake_start(&mut self) {
        self.inflight_handshakes += 1u8;
    }

    pub fn on_handshake_end(&mut self) {
        self.inflight_handshakes -= 1u8;
    }

    pub fn inflight_handshakes(&self) -> usize {
        *self.inflight_handshakes
    }
}
