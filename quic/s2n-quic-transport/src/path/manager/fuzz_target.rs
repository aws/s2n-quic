// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{tests::helper_path, *};
use crate::{
    connection::{ConnectionIdMapper, InternalConnectionIdGenerator},
    endpoint::testing::Server as Config,
    path,
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
};
use std::collections::HashMap;

type Manager = super::Manager<Config>;
type Handle = path::RemoteAddress;

/// Path information stored for modeling the path::Manager
#[derive(Debug)]
struct PathInfo {
    state: path::State,
    handle: Handle,
}

impl PathInfo {
    pub fn new(path: Path<Config>) -> PathInfo {
        PathInfo {
            handle: path.handle,
            state: path.state,
        }
    }

    fn remote_address(&self) -> Handle {
        self.handle
    }

    fn is_validated(&self) -> bool {
        self.state == path::State::Validated
    }
}

#[derive(Debug)]
struct Oracle {
    paths: HashMap<Handle, PathInfo>,
    active: Handle,
    last_known_active_validated_path: Option<u8>,
    prev_state_amplification_limited: bool,
}

impl Oracle {
    fn new(handle: Handle, path: PathInfo) -> Oracle {
        let mut paths = HashMap::new();
        paths.insert(handle, path);

        Oracle {
            paths,
            active: handle,
            last_known_active_validated_path: None,
            prev_state_amplification_limited: false,
        }
    }

    fn get_active_path(&self) -> &PathInfo {
        self.paths.get(&self.active).unwrap()
    }

    fn path(&mut self, handle: &Handle) -> Option<(&Handle, &mut PathInfo)> {
        self.paths
            .iter_mut()
            .find(|(path_handle, _path)| path_handle.eq(&handle))
    }

    /// Insert a new path while adhering to limits
    fn insert_new_path(&mut self, new_handle: Handle, new_path: PathInfo) {
        // We only store max of 4 paths because of limits imposed by Active cid limits
        if self.paths.len() < 4 {
            self.paths.insert(new_handle, new_path);
        }
    }

    /// Insert new path if the remote address doesnt match any existing path
    fn on_datagram_received(
        &mut self,
        handle: &Handle,
        payload_len: u16,
        still_amplification_limited: bool,
    ) {
        match self.path(handle) {
            Some(_path) => (),
            None => {
                let new_path = PathInfo {
                    handle: *handle,
                    state: path::State::Validated,
                };

                self.insert_new_path(*handle, new_path);
            }
        }

        // Verify receiving bytes unblocks amplification limited.
        //
        // Amplification is calculated in terms of packets rather than bytes. Therefore any
        // payload len that is > 0 will unblock amplification.
        if self.prev_state_amplification_limited && payload_len > 0 {
            assert!(!still_amplification_limited);
        }
    }

    fn on_timeout(&self, _millis: &u16) {
        // TODO implement
        // call timeout on each Path
        //
        // if active path is not validated and validation failed
        //   set the last_known_active_validated_path as active path
    }

    fn on_processed_packet(&mut self, handle: &Handle, probe: &path_validation::Probe) {
        if !probe.is_probing() {
            self.active = *handle;
        }
    }
}

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

    // on_timeout
    IncrementTime {
        /// The milli-second value by which to increase the timestamp
        millis: u16,
    },
}

#[derive(Debug)]
struct Model {
    oracle: Oracle,
    subject: Manager,

    /// A monotonically increasing timestamp
    timestamp: Timestamp,
}

impl Model {
    pub fn new() -> Model {
        let zero_conn_id = connection::PeerId::try_from_bytes(&[0]).unwrap();
        let new_addr = Handle::default();
        let oracle = {
            let zero_path = PathInfo::new(helper_path(zero_conn_id));
            Oracle::new(new_addr, zero_path)
        };

        let manager = {
            let zero_path = helper_path(zero_conn_id);
            let mut random_generator = random::testing::Generator(123);
            let mut peer_id_registry =
                ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
                    .create_server_peer_id_registry(
                        InternalConnectionIdGenerator::new().generate_id(),
                        zero_path.peer_connection_id,
                    );

            // register 3 more ids which means a total of 4 paths. cid 0 is retired as part of
            // on_new_connection_id call
            for i in 1..=3 {
                let cid = connection::PeerId::try_from_bytes(&[i]).unwrap();
                let token = &[i; 16].into();
                peer_id_registry
                    .on_new_connection_id(&cid, i.into(), 0, token)
                    .unwrap();
            }

            Manager::new(zero_path, peer_id_registry)
        };

        let clock = Clock::default();
        Model {
            oracle,
            subject: manager,
            timestamp: clock.get_time(),
        }
    }

    pub fn apply(&mut self, operation: &Operation) {
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
        }
    }

    fn on_timeout(&mut self, millis: &u16) {
        // timestamp should be monotonically increasing
        self.timestamp += Duration::from_millis(*millis as u64);

        self.oracle.on_timeout(millis);

        self.subject
            .on_timeout(
                self.timestamp,
                &mut Generator::default(),
                &mut Publisher::no_snapshot(),
            )
            .unwrap();
    }

    fn on_datagram_received(
        &mut self,
        handle: &Handle,
        probing: &Option<path_validation::Probe>,
        local_id: LocalId,
        payload_len: u16,
    ) {
        // Handle is an alias to RemoteAddress so does not inherit the PathHandle
        // eq implementation which unmaps an ipv6 address into a ipv4 address
        let handle = path::RemoteAddress(handle.unmap());
        let datagram = DatagramInfo {
            timestamp: self.timestamp,
            payload_len: payload_len as usize,
            ecn: ExplicitCongestionNotification::NotEct,
            destination_connection_id: local_id,
            source_connection_id: None,
        };
        let mut migration_validator = path::migration::default::Validator;
        let mut random_generator = Generator::default();
        let mut publisher = Publisher::no_snapshot();

        self.oracle.prev_state_amplification_limited = self
            .subject
            .path(&handle)
            .map_or(false, |(_id, path)| path.at_amplification_limit());

        match self.subject.on_datagram_received(
            &handle,
            &datagram,
            true,
            &mut Default::default(),
            &mut migration_validator,
            MaxMtu::default(),
            &mut publisher,
        ) {
            Ok((id, _)) => {
                // Only call oracle if the subject can process on_datagram_received without errors
                self.oracle.on_datagram_received(
                    &handle,
                    payload_len,
                    self.subject[id].at_amplification_limit(),
                );

                if let Some(probe) = probing {
                    self.oracle.on_processed_packet(&handle, probe);

                    let (path_id, _path) = self.subject.path(&handle).unwrap();
                    self.subject
                        .on_processed_packet(
                            path_id,
                            None,
                            *probe,
                            &mut random_generator,
                            &mut publisher,
                        )
                        .unwrap();
                }
            }
            Err(err) => {
                // Ignore errors emitted by the migration::validator and peer_id_registry
                let ignore_err = err.reason == "insufficient connection ids"
                    || err.reason == "migration attempt denied";
                if !ignore_err {
                    panic!("{}", err)
                }
            }
        }
    }

    fn on_bytes_transmitted(&mut self, path_id_generator: u8, bytes: u16) {
        let path_id = path_id_generator as usize % self.subject.paths.len();

        let path = &mut self.subject[Id(path_id as u8)];
        if !path.at_amplification_limit() {
            path.on_bytes_transmitted(bytes as usize);
        }
    }

    /// Check that the subject and oracle match.
    pub fn invariants(&self) {
        // compare total paths
        assert_eq!(self.oracle.paths.len(), self.subject.paths.len());

        // compare active path
        assert_eq!(
            self.oracle.get_active_path().remote_address(),
            self.subject.active_path().remote_address()
        );

        // compare last known valid path
        assert_eq!(
            self.oracle.last_known_active_validated_path,
            self.subject.last_known_active_validated_path
        );

        // compare path properties
        for (path_id, s_path) in self.subject.paths.iter().enumerate() {
            let o_path = self.oracle.paths.get(&s_path.remote_address()).unwrap();

            assert_eq!(
                o_path.remote_address(),
                s_path.remote_address(),
                "path_id: {}",
                path_id
            );
            assert_eq!(
                o_path.is_validated(),
                s_path.is_validated(),
                "path_id: {}",
                path_id
            );
        }
    }
}

#[test]
fn cm_model_test() {
    check!()
        .with_type::<Vec<Operation>>()
        .for_each(|operations| {
            let mut model = Model::new();
            for operation in operations.iter() {
                model.apply(operation);
            }

            model.invariants();
        })
}
