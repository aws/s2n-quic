// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{tests::helper_path, *};
use crate::{
    connection::{ConnectionIdMapper, InternalConnectionIdGenerator},
    endpoint::testing::Server as Config,
};
use bolero::{check, generator::*};
use core::time::Duration;
use s2n_quic_core::{
    connection::LocalId,
    event::testing::Publisher,
    frame::path_validation,
    inet::{DatagramInfo, ExplicitCongestionNotification},
    random,
    random::testing::Generator,
    time::{testing::Clock, Clock as _},
    transport,
};

type Manager = super::Manager<Config>;
type Handle = path::RemoteAddress;

#[derive(Debug, TypeGenerator)]
enum Operation {
    // on_datagram_received
    // on_processed_packet
    OnDatagramReceived {
        /// Used for calling on_processed_packet
        ///
        /// `Some` implies that the packet was successfully processed
        /// and that `on_processed_packet` should be called.
        probing: Option<path_validation::Probe>,

        /// The remote address from which the datagram was sent
        handle: Handle,

        /// The peer may switch the cid on the connection
        local_id: LocalId,

        // 9000 captures up to jumbo frames
        #[generator(0..9000)]
        payload_len: u16,
    },

    // path.on_bytes_transmitted
    OnBytesTransmit {
        /// Used to calculate path_id
        ///
        /// path_id = path_id_generator % paths.len()
        #[generator(1..100)]
        path_id_generator: u8,

        /// Bytes to be transmitted
        // 9000 captures up to jumbo frames
        #[generator(0..9000)]
        bytes: u16,
    },

    OnNewConnectionId {
        sequence_number: u32,
        retire_prior_to: u32,
        id: connection::PeerId,
        stateless_reset_token: stateless_reset::Token,
    },

    // on_timeout
    IncrementTime {
        /// The milli-second value by which to increase the timestamp
        millis: u16,
    },
}

#[derive(Debug)]
struct Model {
    subject: Manager,

    /// A monotonically increasing timestamp
    timestamp: Timestamp,
}

impl Model {
    pub fn new() -> Model {
        let zero_conn_id = connection::PeerId::try_from_bytes(&[0]).unwrap();

        let manager = {
            let zero_path = helper_path(zero_conn_id);
            let mut random_generator = random::testing::Generator(123);
            let peer_id_registry =
                ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
                    .create_server_peer_id_registry(
                        InternalConnectionIdGenerator::new().generate_id(),
                        zero_path.peer_connection_id,
                        true,
                    );

            Manager::new(zero_path, peer_id_registry)
        };

        let clock = Clock::default();
        Model {
            subject: manager,
            timestamp: clock.get_time(),
        }
    }

    pub fn apply(&mut self, operation: &Operation) -> Result<(), transport::Error> {
        match operation {
            Operation::IncrementTime { millis } => self.on_timeout(millis),
            Operation::OnDatagramReceived {
                probing,
                handle,
                local_id,
                payload_len,
            } => self.on_datagram_received(handle, probing, *local_id, *payload_len),
            Operation::OnBytesTransmit {
                path_id_generator,
                bytes,
            } => self.on_bytes_transmitted(*path_id_generator, *bytes),
            Operation::OnNewConnectionId {
                id,
                sequence_number,
                retire_prior_to,
                stateless_reset_token,
            } => self.on_new_connection_id(
                id,
                *sequence_number,
                *retire_prior_to,
                *stateless_reset_token,
            ),
        }
    }

    fn on_timeout(&mut self, millis: &u16) -> Result<(), transport::Error> {
        // timestamp should be monotonically increasing
        self.timestamp += Duration::from_millis(*millis as u64);

        let _ = self
            .subject
            .on_timeout(
                self.timestamp,
                &mut Generator::default(),
                &mut Publisher::no_snapshot(),
            )
            .unwrap();

        Ok(())
    }

    fn on_datagram_received(
        &mut self,
        handle: &Handle,
        probing: &Option<path_validation::Probe>,
        local_id: LocalId,
        payload_len: u16,
    ) -> Result<(), transport::Error> {
        // Handle is an alias to RemoteAddress so does not inherit the PathHandle
        // eq implementation which unmaps an ipv6 address into a ipv4 address
        let handle = path::RemoteAddress(handle.unmap());
        let datagram = DatagramInfo {
            timestamp: self.timestamp,
            payload_len: payload_len as usize,
            ecn: ExplicitCongestionNotification::NotEct,
            destination_connection_id: local_id,
            destination_connection_id_classification: connection::id::Classification::Local,
            source_connection_id: None,
        };
        let mut migration_validator = path::migration::allow_all::Validator;
        let mut random_generator = Generator::default();
        let mut publisher = Publisher::no_snapshot();

        match self.subject.on_datagram_received(
            &handle,
            &datagram,
            true,
            &mut Default::default(),
            &mut migration_validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        ) {
            Ok(_) => {
                if let Some(probe) = probing {
                    let (path_id, _path) = self.subject.path(&handle).unwrap();
                    let _ = self.subject.on_processed_packet(
                        path_id,
                        None,
                        *probe,
                        &mut random_generator,
                        &mut publisher,
                    )?;
                }
            }
            Err(datagram_drop_reason) => {
                match datagram_drop_reason {
                    // Ignore errors emitted by the migration::validator and peer_id_registry
                    DatagramDropReason::InsufficientConnectionIds => {}
                    DatagramDropReason::RejectedConnectionMigration { .. } => {}
                    DatagramDropReason::PathLimitExceeded => {}
                    datagram_drop_reason => panic!("{datagram_drop_reason:?}"),
                };
            }
        }

        Ok(())
    }

    fn on_new_connection_id(
        &mut self,
        connection_id: &connection::PeerId,
        sequence_number: u32,
        retire_prior_to: u32,
        stateless_reset_token: stateless_reset::Token,
    ) -> Result<(), transport::Error> {
        // These validations are performed when decoding the `NEW_CONNECTION_ID` frame
        // see s2n-quic-core/src/frame/new_connection_id.rs
        if retire_prior_to > sequence_number {
            return Ok(());
        }

        if !((1..=20).contains(&connection_id.len())) {
            return Ok(());
        }

        if sequence_number == 0 {
            return Ok(());
        }

        let mut publisher = Publisher::no_snapshot();

        self.subject.on_new_connection_id(
            connection_id,
            sequence_number,
            retire_prior_to,
            &stateless_reset_token,
            &mut publisher,
        )
    }

    fn on_bytes_transmitted(
        &mut self,
        path_id_generator: u8,
        bytes: u16,
    ) -> Result<(), transport::Error> {
        let id = path_id_generator as usize % self.subject.paths.len();

        let path = &mut self.subject[path_id(id as u8)];
        if !path.at_amplification_limit() {
            path.on_bytes_transmitted(bytes as usize);
        }

        Ok(())
    }

    /// Check that the subject and oracle match.
    pub fn invariants(&self) {
        let peer_id = self.subject.active_path().peer_connection_id;

        // The active path should be using an active connection ID
        assert!(self.subject.peer_id_registry.is_active(&peer_id));
    }
}

#[test]
fn cm_model_test() {
    check!()
        .with_type::<Vec<Operation>>()
        .for_each(|operations| {
            let mut model = Model::new();
            let mut error = false;

            for operation in operations.iter() {
                if model.apply(operation).is_err() {
                    error = true;
                    break;
                }
            }

            if !error {
                model.invariants();
            }
        })
}
