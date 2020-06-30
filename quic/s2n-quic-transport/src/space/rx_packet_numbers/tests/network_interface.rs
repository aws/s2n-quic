use super::{generator::gen_default, Application, Packet};
use alloc::collections::BTreeMap;
use bolero::generator::*;
use s2n_quic_core::{endpoint::EndpointType, time::Timestamp};

#[derive(Clone, Debug, TypeGenerator)]
pub struct NetworkInterface {
    pub application: Application,
    #[generator(gen_default())]
    rx_queue: BTreeMap<Timestamp, Packet>,
}

impl NetworkInterface {
    pub fn new(application: Application) -> Self {
        Self {
            application,
            rx_queue: Default::default(),
        }
    }

    pub fn init(&mut self, now: Timestamp, endpoint_type: EndpointType) {
        self.application.init(now, endpoint_type);
    }

    pub fn recv(&mut self, packet: Packet) {
        self.rx_queue.insert(packet.time, packet);
    }

    pub fn tick(&mut self, now: Timestamp) -> Option<Packet> {
        if let Some(packet) = self.rx_queue.remove(&now) {
            self.application.recv(packet);
        }

        self.application.tick(now)
    }

    pub fn next_tick(&self) -> Option<Timestamp> {
        self.timers().min().cloned()
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.application.timers().chain(self.rx_queue.keys().next())
    }

    pub fn done(&mut self) {
        assert_eq!(self.rx_queue.len(), 0);
        self.application.done();
    }
}

impl Into<NetworkInterface> for Application {
    fn into(self) -> NetworkInterface {
        NetworkInterface::new(self)
    }
}
