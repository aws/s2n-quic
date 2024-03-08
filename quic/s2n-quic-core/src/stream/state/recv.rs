// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

//= https://www.rfc-editor.org/rfc/rfc9000#section-3.2
//#        o
//#       | Recv STREAM / STREAM_DATA_BLOCKED / RESET_STREAM
//#       | Create Bidirectional Stream (Sending)
//#       | Recv MAX_STREAM_DATA / STOP_SENDING (Bidirectional)
//#       | Create Higher-Numbered Stream
//#       v
//#   +-------+
//#   | Recv  | Recv RESET_STREAM
//#   |       |-----------------------.
//#   +-------+                       |
//#       |                           |
//#       | Recv STREAM + FIN         |
//#       v                           |
//#   +-------+                       |
//#   | Size  | Recv RESET_STREAM     |
//#   | Known |---------------------->|
//#   +-------+                       |
//#       |                           |
//#       | Recv All Data             |
//#       v                           v
//#   +-------+ Recv RESET_STREAM +-------+
//#   | Data  |--- (optional) --->| Reset |
//#   | Recvd |  Recv All Data    | Recvd |
//#   +-------+<-- (optional) ----+-------+
//#       |                           |
//#       | App Read All Data         | App Read Reset
//#       v                           v
//#   +-------+                   +-------+
//#   | Data  |                   | Reset |
//#   | Read  |                   | Read  |
//#   +-------+                   +-------+

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Receiver {
    #[default]
    Recv,
    SizeKnown,
    DataRecvd,
    DataRead,
    ResetRecvd,
    ResetRead,
}

impl Receiver {
    is!(Recv, is_receiving);
    is!(SizeKnown, is_size_known);
    is!(DataRecvd, is_data_received);
    is!(DataRead, is_data_read);
    is!(ResetRecvd, is_reset_received);
    is!(ResetRead, is_reset_read);
    is!(DataRead | ResetRead, is_terminal);

    #[inline]
    pub fn on_receive_fin(&mut self) -> Result<Self> {
        use Receiver::*;
        transition!(self,  Recv => SizeKnown)
    }

    #[inline]
    pub fn on_receive_all_data(&mut self) -> Result<Self> {
        use Receiver::*;
        transition!(self, SizeKnown => DataRecvd)
    }

    #[inline]
    pub fn on_app_read_all_data(&mut self) -> Result<Self> {
        use Receiver::*;
        transition!(self, DataRecvd => DataRead)
    }

    #[inline]
    pub fn on_reset(&mut self) -> Result<Self> {
        use Receiver::*;
        transition!(self, Recv | SizeKnown => ResetRecvd)
    }

    #[inline]
    pub fn on_app_read_reset(&mut self) -> Result<Self> {
        use Receiver::*;
        transition!(self, ResetRecvd => ResetRead)
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
            Receiver::Recv,
            Receiver::SizeKnown,
            Receiver::DataRecvd,
            Receiver::DataRead,
            Receiver::ResetRecvd,
            Receiver::ResetRead,
        ];
        for state in states {
            macro_rules! push {
                ($event:ident) => {
                    let mut target = state.clone();
                    let result = target.$event().map(|_| target);
                    outcomes.push((state.clone(), stringify!($event), result));
                };
            }
            push!(on_receive_fin);
            push!(on_receive_all_data);
            push!(on_app_read_all_data);
            push!(on_reset);
            push!(on_app_read_reset);
        }

        assert_debug_snapshot!(outcomes);
    }
}
