// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::testing::*,
    endpoint,
    space::HandshakeStatus,
    transmission::{self, interest::Provider},
};
use bolero::{check, generator::*};
use s2n_quic_core::{ack, event::testing::Publisher, packet, time::clock::testing as time};

#[derive(Debug)]
struct Oracle {
    pending: bool,
    complete: bool,
    confirmed: bool,
    endpoint_type: endpoint::Type,

    // SERVER
    handshake_done_sent: bool,
    handshake_done_delivered: bool,

    // Client
    handshake_done_received: bool,
}

impl Oracle {
    fn new(endpoint_type: endpoint::Type) -> Self {
        Oracle {
            pending: true,
            complete: false,
            confirmed: false,
            endpoint_type,
            handshake_done_sent: false,
            handshake_done_delivered: false,
            handshake_done_received: false,
        }
    }

    fn can_transmit(&self) -> bool {
        // if the server endpoint is complete but has not yet
        // delivered the HANDSHAKE_DONE
        if self.endpoint_type.is_server() && self.complete && !self.handshake_done_delivered {
            return true;
        }

        false
    }

    fn on_handshake_complete(&mut self) {
        self.pending = false;
        self.complete = true;
        if self.endpoint_type.is_server() {
            self.confirmed = true;
        }
    }

    fn on_transmit(&mut self) {
        if self.endpoint_type.is_client() {
            return;
        }

        self.handshake_done_sent = true;
    }

    fn on_packet_ack(&mut self, ack_handshake_done: bool) {
        if self.endpoint_type.is_client() {
            return;
        }

        // check that the HANDSHAKE_DONE frame was sent
        if self.complete && self.handshake_done_sent && ack_handshake_done {
            self.handshake_done_delivered = true;
        }
    }

    fn on_packet_loss(&mut self, lost_handshake_done: bool) {
        if self.endpoint_type.is_client() {
            return;
        }

        // check that the HANDSHAKE_DONE frame was sent but not yet delivered
        if self.handshake_done_sent && !self.handshake_done_delivered && lost_handshake_done {
            self.handshake_done_sent = false;
        }
    }

    fn on_handshake_done_received(&mut self) {
        if self.endpoint_type.is_server() {
            return;
        }

        if self.complete {
            self.confirmed = true;
            self.handshake_done_received = true;
        }
    }
}

#[derive(Debug, TypeGenerator)]
enum Operation {
    Complete,

    // SERVER: below are server specific operations
    TransmitHandshakeDone {},
    AckHandshakeDonePacket { ack_handshake_done: bool },
    LostHandshakeDonePacket { lost_handshake_done: bool },

    // Client: below are client specific operations
    ReceivedHandshakeDone {},
}

struct Model {
    subject: HandshakeStatus,
    oracle: Oracle,
}

impl Model {
    fn new(endpoint_type: endpoint::Type) -> Self {
        Model {
            subject: HandshakeStatus::InProgress,
            oracle: Oracle::new(endpoint_type),
        }
    }

    fn apply(&mut self, operation: &Operation) {
        match operation {
            Operation::Complete => self.on_complete(),
            Operation::TransmitHandshakeDone {} => self.packet_transmit(),
            Operation::AckHandshakeDonePacket { ack_handshake_done } => {
                self.packet_acked(*ack_handshake_done)
            }
            Operation::LostHandshakeDonePacket {
                lost_handshake_done,
            } => self.packet_loss(*lost_handshake_done),
            Operation::ReceivedHandshakeDone {} => self.on_handshake_done_received(),
        }
    }

    fn on_complete(&mut self) {
        if !self.oracle.complete {
            self.subject
                .on_handshake_complete(self.oracle.endpoint_type, &mut Publisher::no_snapshot());
            self.oracle.on_handshake_complete();
        }
    }

    fn packet_transmit(&mut self) {
        if self.oracle.can_transmit() {
            assert!(matches!(
                self.subject,
                HandshakeStatus::ServerCompleteConfirmed(_)
            ));

            let mut frame_buffer = OutgoingFrameBuffer::new();
            let mut context = MockWriteContext::new(
                time::now(),
                &mut frame_buffer,
                // always allow for transmission for the purpose of this test
                transmission::Constraint::None,
                transmission::Mode::Normal,
                self.oracle.endpoint_type,
            );
            self.subject.on_transmit(&mut context);
            // verify that the subject wrote a frame
            assert!(!frame_buffer.is_empty());

            self.oracle.on_transmit();
        }
    }

    fn packet_acked(&mut self, ack_handshake_done: bool) {
        self.subject.on_packet_ack(
            &AckSetMock(ack_handshake_done),
            &mut Publisher::no_snapshot(),
        );
        self.oracle.on_packet_ack(ack_handshake_done);
    }

    fn packet_loss(&mut self, lost_handshake_done: bool) {
        self.subject.on_packet_loss(
            &AckSetMock(lost_handshake_done),
            &mut Publisher::no_snapshot(),
        );

        // perform some checks before calling `oracle.on_packet_loss`
        if self.oracle.endpoint_type.is_server() {
            // check that the HANDSHAKE_DONE frame was sent but not yet delivered
            if self.oracle.handshake_done_sent && !self.oracle.handshake_done_delivered {
                assert!(matches!(
                    self.subject,
                    HandshakeStatus::ServerCompleteConfirmed(_)
                ));

                if lost_handshake_done {
                    if let HandshakeStatus::ServerCompleteConfirmed(flag) = &self.subject {
                        //= https://www.rfc-editor.org/rfc/rfc9000#section-13.3
                        //= type=test
                        //# The HANDSHAKE_DONE frame MUST be retransmitted until it is
                        //# acknowledged.
                        assert!(flag.has_transmission_interest());
                    }
                }
            }
        }

        self.oracle.on_packet_loss(lost_handshake_done);
    }

    fn on_handshake_done_received(&mut self) {
        self.subject
            .on_handshake_done_received(&mut Publisher::no_snapshot());
        self.oracle.on_handshake_done_received();
    }

    fn invariants(&self) {
        assert_eq!(self.subject.is_complete(), self.oracle.complete);
        assert_eq!(self.subject.is_confirmed(), self.oracle.confirmed);

        assert_eq!(
            self.oracle.pending,
            matches!(self.subject, HandshakeStatus::InProgress)
        );
        if matches!(self.subject, HandshakeStatus::Confirmed) {
            assert!(self.oracle.complete);
            assert!(self.oracle.confirmed);
        }

        // SERVER
        if self.oracle.endpoint_type.is_server() {
            if self.oracle.complete {
                assert!(self.subject.is_confirmed());
            }

            if self.oracle.handshake_done_sent {
                assert!(!matches!(self.subject, HandshakeStatus::InProgress));
                if let HandshakeStatus::ServerCompleteConfirmed(flag) = &self.subject {
                    // HANDSHAKE_DONE should either needs transmission or is in-flight
                    assert!(!flag.is_idle());
                }
            }
            if self.oracle.handshake_done_delivered {
                assert!(matches!(self.subject, HandshakeStatus::Confirmed));
            }
        }

        // CLIENT
        if self.oracle.endpoint_type.is_client() {
            // If handshake is complete but awaiting confirmation
            if self.subject.is_complete() && !self.oracle.handshake_done_received {
                assert!(!self.subject.is_confirmed());
                assert!(matches!(self.subject, HandshakeStatus::ClientComplete));
            }

            if self.oracle.handshake_done_received {
                assert!(self.subject.is_confirmed());
            }
        }
    }
}

struct AckSetMock(bool);
impl ack::Set for AckSetMock {
    fn contains(&self, _packet_number: packet::number::PacketNumber) -> bool {
        self.0
    }

    fn smallest(&self) -> packet::number::PacketNumber {
        todo!("unused for this test")
    }

    fn largest(&self) -> packet::number::PacketNumber {
        todo!("unused for this test")
    }
}

#[test]
fn handshake_status_fuzz() {
    check!()
        .with_type::<Vec<Operation>>()
        .for_each(|operations| {
            let mut server_model = Model::new(endpoint::Type::Server);
            let mut client_model = Model::new(endpoint::Type::Client);

            for operation in operations.iter() {
                server_model.apply(operation);
                client_model.apply(operation);
            }

            server_model.invariants();
            client_model.invariants();
        });
}
