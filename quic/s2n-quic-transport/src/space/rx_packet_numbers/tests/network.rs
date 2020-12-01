use super::{NetworkEvent, NetworkInterface, Report};
use bolero::generator::*;
use core::time::Duration;
use s2n_quic_core::{endpoint, time::Timestamp};

#[derive(Clone, Debug, TypeGenerator)]
pub struct Network {
    pub client: NetworkInterface,
    pub server: NetworkInterface,
}

impl Network {
    pub fn init(&mut self, now: Timestamp) {
        self.client.init(now, endpoint::Type::Client);
        self.server.init(now, endpoint::Type::Server);
    }

    pub fn tick<E: Iterator<Item = NetworkEvent>>(
        &mut self,
        now: Timestamp,
        events: &mut E,
        network_delay: Duration,
        report: &mut Report,
    ) {
        macro_rules! transmit {
            ($from:ident, $to:ident) => {
                if let Some(packet) = self.$from.tick(now) {
                    if let Some(mut packet) = events
                        .next()
                        .unwrap_or(NetworkEvent::Pass)
                        .process_packet(packet, &mut report.$from)
                    {
                        // add standard network delay
                        packet.time += network_delay;
                        self.$to.recv(packet);
                    }
                }
            };
        }

        transmit!(client, server);
        transmit!(server, client);
    }

    pub fn next_tick(&self) -> Option<Timestamp> {
        self.timers().min().cloned()
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.client.timers().chain(self.server.timers())
    }

    pub fn done(&mut self) {
        self.client.done();
        self.server.done();
    }
}
