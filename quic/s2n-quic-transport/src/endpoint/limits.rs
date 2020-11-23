pub struct Manager {
    inflight_handshakes: usize,
}

impl Manager {
    pub fn new() -> Self {
        Self {
            inflight_handshakes: 0,
        }
    }

    pub fn on_handshake_start(&mut self) {
        self.inflight_handshakes += 1;
    }

    pub fn on_handshake_end(&mut self) {
        self.inflight_handshakes -= 1;
    }

    pub fn inflight_handshakes(&self) -> usize {
        self.inflight_handshakes
    }
}
