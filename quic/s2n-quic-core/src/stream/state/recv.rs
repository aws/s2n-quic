// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::state::{event, is};

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
    is!(is_receiving, Recv);
    is!(is_size_known, SizeKnown);
    is!(is_data_received, DataRecvd);
    is!(is_data_read, DataRead);
    is!(is_reset_received, ResetRecvd);
    is!(is_reset_read, ResetRead);
    is!(is_terminal, DataRead | ResetRead);

    event! {
        on_receive_fin(Recv => SizeKnown);
        on_receive_all_data(SizeKnown => DataRecvd);
        on_app_read_all_data(DataRecvd => DataRead);

        on_reset(Recv | SizeKnown => ResetRecvd);
        on_app_read_reset(ResetRecvd => ResetRead);
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

    #[test]
    #[cfg_attr(miri, ignore)]
    fn dot_test() {
        assert_snapshot!(Receiver::dot());
    }
}
