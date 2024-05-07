// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::state::{event, is};

//= https://www.rfc-editor.org/rfc/rfc9000#section-3.1
//#        o
//#       | Create Stream (Sending)
//#       | Peer Creates Bidirectional Stream
//#       v
//#   +-------+
//#   | Ready | Send RESET_STREAM
//#   |       |-----------------------.
//#   +-------+                       |
//#       |                           |
//#       | Send STREAM /             |
//#       |      STREAM_DATA_BLOCKED  |
//#       v                           |
//#   +-------+                       |
//#   | Send  | Send RESET_STREAM     |
//#   |       |---------------------->|
//#   +-------+                       |
//#       |                           |
//#       | Send STREAM + FIN         |
//#       v                           v
//#   +-------+                   +-------+
//#   | Data  | Send RESET_STREAM | Reset |
//#   | Sent  |------------------>| Sent  |
//#   +-------+                   +-------+
//#       |                           |
//#       | Recv All ACKs             | Recv ACK
//#       v                           v
//#   +-------+                   +-------+
//#   | Data  |                   | Reset |
//#   | Recvd |                   | Recvd |
//#   +-------+                   +-------+

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Sender {
    #[default]
    Ready,
    Send,
    DataSent,
    DataRecvd,
    /// An additional state for implementations to separate queueing a RESET_STREAM from actually
    /// sending it
    ResetQueued,
    ResetSent,
    ResetRecvd,
}

impl Sender {
    is!(is_ready, Ready);
    is!(is_sending, Send);
    is!(is_data_sent, DataSent);
    is!(is_data_received, DataRecvd);
    is!(is_reset_queued, ResetQueued);
    is!(is_reset_sent, ResetSent);
    is!(is_reset_received, ResetRecvd);
    is!(is_terminal, DataRecvd | ResetRecvd);

    event! {
        on_send_stream(Ready => Send);
        // we can jump from Ready to DataSent even though the
        // diagram doesn't explicitly highlight this transition
        on_send_fin(Ready | Send => DataSent);
        on_recv_all_acks(DataSent | ResetQueued => DataRecvd);

        on_queue_reset(Ready | Send | DataSent => ResetQueued);
        on_send_reset(Ready | Send | DataSent | ResetQueued => ResetSent);
        on_recv_reset_ack(ResetSent => ResetRecvd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_debug_snapshot, assert_snapshot};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn snapshots() {
        let mut outcomes = vec![];
        let states = [
            Sender::Ready,
            Sender::Send,
            Sender::DataSent,
            Sender::DataRecvd,
            Sender::ResetQueued,
            Sender::ResetSent,
            Sender::ResetRecvd,
        ];
        for state in states {
            macro_rules! push {
                ($event:ident) => {
                    let mut target = state.clone();
                    let result = target.$event().map(|_| target);
                    outcomes.push((state.clone(), stringify!($event), result));
                };
            }
            push!(on_send_stream);
            push!(on_send_fin);
            push!(on_queue_reset);
            push!(on_send_reset);
            push!(on_recv_all_acks);
            push!(on_recv_reset_ack);
        }

        assert_debug_snapshot!(outcomes);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn dot_test() {
        assert_snapshot!(Sender::dot());
    }
}
