// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

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
    is!(Ready, is_ready);
    is!(Send, is_sending);
    is!(DataSent, is_data_sent);
    is!(DataRecvd, is_data_received);
    is!(ResetQueued, is_reset_queued);
    is!(ResetSent, is_reset_sent);
    is!(ResetRecvd, is_reset_received);
    is!(DataRecvd | ResetRecvd, is_terminal);

    #[inline]
    pub fn on_send_stream(&mut self) -> Result<Self> {
        use Sender::*;
        transition!(self,  Ready => Send)
    }

    #[inline]
    pub fn on_send_fin(&mut self) -> Result<Self> {
        use Sender::*;
        // we can jump from Ready to DataSent even though the
        // diagram doesn't explicitly highlight this transition
        transition!(self,  Ready | Send => DataSent)
    }

    #[inline]
    pub fn on_queue_reset(&mut self) -> Result<Self> {
        use Sender::*;
        transition!(self, Ready | Send | DataSent => ResetQueued)
    }

    #[inline]
    pub fn on_send_reset(&mut self) -> Result<Self> {
        use Sender::*;
        transition!(self, Ready | Send | DataSent | ResetQueued => ResetSent)
    }

    #[inline]
    pub fn on_recv_all_acks(&mut self) -> Result<Self> {
        use Sender::*;
        transition!(self, DataSent | ResetQueued => DataRecvd)
    }

    #[inline]
    pub fn on_recv_reset_ack(&mut self) -> Result<Self> {
        use Sender::*;
        transition!(self, ResetSent => ResetRecvd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

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
}
