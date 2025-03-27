// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{endpoint, state::event};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// The client is in the initial state
    ///
    /// In this state:
    /// * The client has chosen a local queue_id and is actively transmitting that
    ///   value in the `source_queue_id` field.
    /// * The client is not aware of what the server (peer) has chosen for its `queue_id`.
    ClientInit,

    /// In this state:
    /// * The client has seen at least one packet from the server. This means it is
    ///   now aware of the server's chosen `queue_id`.
    ///
    /// At this point the client can:
    /// * Update the destination `queue_id` to use for future transmissions.
    /// * Stop transmitting its `source_queue_id` since the client was able to observe
    ///   at least one packet from the server with the chosen client `queue_id` value.
    ClientQueueIdObserved,

    /// The server is in the initial state
    ///
    /// In this state:
    /// * The server has received at least one packet from the client. That packet included
    ///   the client's chosen `queue_id`.
    /// * The server has also selected its own `queue_id` and is actively transmitting
    ///   that value in the `source_queue_id` field.
    /// * The peer (client) is not currently aware of our chosen `queue_id` value
    ServerInit,

    /// In this state:
    /// * The server has received a `Stream` packet containing a non-zero value of
    ///   `next_expected_control_packet`, indicating the client has observed at least
    ///   one control packet has been received with the server's chosen `queue_id`.
    /// * The server has received a valid `Control` packet from the client with the
    ///   expected `queue_id`.
    ServerQueueIdObserved,

    /// All needed observations have been made and no further state updates are required.
    Finished,
}

impl State {
    event! {
        /// Called when at least one valid stream packet is received
        on_stream_packet(ClientInit => ClientQueueIdObserved);

        /// Called when at least one valid control packet is received
        on_control_packet(ClientInit => ClientQueueIdObserved, ServerInit => ServerQueueIdObserved);

        /// Called when the `read` half receives a `Stream` packet containing a non-zero
        /// `next_expected_control_packet` value.
        on_non_zero_next_expected_control_packet(ServerInit => ServerQueueIdObserved);

        /// Called when the observations have been recorded and the stream is in a steady state
        on_observation_finished(ClientQueueIdObserved | ServerQueueIdObserved => Finished);
    }
}

impl From<endpoint::Type> for State {
    #[inline]
    fn from(value: endpoint::Type) -> Self {
        match value {
            endpoint::Type::Client => Self::ClientInit,
            endpoint::Type::Server => Self::ServerInit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_debug_snapshot, assert_snapshot};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn snapshots() {
        assert_debug_snapshot!(State::test_transitions());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn dot_test() {
        assert_snapshot!(State::dot());
    }
}
