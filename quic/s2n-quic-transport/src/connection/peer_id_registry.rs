use crate::{
    connection::peer_id_registry::{
        PeerIdRegistrationError::InvalidNewConnectionId,
        PeerIdStatus::{Active, PendingAcknowledgement, PendingRetirement},
    },
    transmission,
    transmission::{Interest, WriteContext},
};
use s2n_quic_core::{ack_set::AckSet, connection, frame, packet::number::PacketNumber};
use smallvec::SmallVec;

/// The amount of ConnectionIds we can register without dynamic memory allocation
const NR_STATIC_REGISTRABLE_IDS: usize = 5;

#[derive(Debug)]
struct PeerIdRegistry {
    /// The connection IDs which are currently registered at the ConnectionIdMapper
    registered_ids: SmallVec<[PeerIdInfo; NR_STATIC_REGISTRABLE_IDS]>,
    retire_prior_to: u32,
}

#[derive(Debug)]
struct PeerIdInfo {
    id: connection::PeerId,
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //# Each Connection ID has an associated sequence number to assist in
    //# detecting when NEW_CONNECTION_ID or RETIRE_CONNECTION_ID frames refer
    //# to the same value.
    sequence_number: u32,
    stateless_reset_token: u128,
    status: PeerIdStatus,
}

/// The current status of the connection ID.
#[derive(Debug, PartialEq)]
enum PeerIdStatus {
    // TODO
    Active,
    PendingAcknowledgement(PacketNumber),
    PendingRetirement(bool),
}

impl PeerIdStatus {
    /// Returns true if this status allows for transmission based on the transmission constraint
    fn can_transmit(&self, constraint: transmission::Constraint) -> bool {
        match self {
            PendingRetirement(true) => constraint.can_retransmit(),
            PendingRetirement(false) => constraint.can_transmit(),
            _ => false,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerIdRegistrationError {
    /// The NEW_CONNECTION_ID frame was invalid
    InvalidNewConnectionId,
}

#[allow(dead_code)]
impl PeerIdRegistry {
    pub fn new(initial_connection_id: &connection::PeerId) -> Self {
        let mut registry = Self {
            registered_ids: SmallVec::new(),
            retire_prior_to: 0,
        };
        registry.registered_ids.push(PeerIdInfo {
            id: *initial_connection_id,
            sequence_number: 0,       // TODO
            stateless_reset_token: 0, // TODO
            status: PeerIdStatus::Active,
        });

        registry
    }

    pub fn on_new_connection_id(
        &mut self,
        id: &connection::PeerId,
        sequence_number: u32,
        retire_prior_to: u32,
        stateless_reset_token: u128,
    ) -> Result<(), PeerIdRegistrationError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
        //# Receipt of the same frame multiple times MUST NOT be treated as a
        //# connection error.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
        //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
        //# previously issued connection ID with a different Stateless Reset
        //# Token or a different sequence number, or if a sequence number is used
        //# for different connection IDs, the endpoint MAY treat that receipt as
        //# a connection error of type PROTOCOL_VIOLATION.
        let same_id_diff_token_or_seq_num = |id_info: &PeerIdInfo| {
            id_info.id == *id
                && (id_info.stateless_reset_token != stateless_reset_token
                    || id_info.sequence_number != sequence_number)
        };
        let diff_id_same_seq_num =
            |id_info: &PeerIdInfo| id_info.id != *id && id_info.sequence_number == sequence_number;

        if self
            .registered_ids
            .iter()
            .any(|id_info| same_id_diff_token_or_seq_num(id_info) || diff_id_same_seq_num(id_info))
        {
            return Err(InvalidNewConnectionId);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
        //# A receiver MUST ignore any Retire Prior To fields that do not
        //# increase the largest received Retire Prior To value.
        self.retire_prior_to = self.retire_prior_to.max(retire_prior_to);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
        //# Upon receipt of an increased Retire Prior To field, the peer MUST
        //# stop using the corresponding connection IDs and retire them with
        //# RETIRE_CONNECTION_ID frames before adding the newly provided
        //# connection ID to the set of active connection IDs.

        self.registered_ids.push(PeerIdInfo {
            id: *id,
            sequence_number,
            stateless_reset_token, // TODO
            status: Active,
        });

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
        //# An endpoint that receives a NEW_CONNECTION_ID frame with a sequence
        //# number smaller than the Retire Prior To field of a previously
        //# received NEW_CONNECTION_ID frame MUST send a corresponding
        //# RETIRE_CONNECTION_ID frame that retires the newly received connection
        //# ID, unless it has already done so for that sequence number.
        let max_retire_prior_to = self.retire_prior_to;

        for mut id_info in self
            .registered_ids
            .iter_mut()
            .filter(|id_info| !matches!(id_info.status, PendingAcknowledgement(_)))
            .filter(|id_info| id_info.sequence_number < max_retire_prior_to)
        {
            id_info.status = PendingRetirement(false);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //= type=TODO
        //= tracking-issue=239
        //= feature=Peer Connection ID Management
        //# After processing a NEW_CONNECTION_ID frame and
        //# adding and retiring active connection IDs, if the number of active
        //# connection IDs exceeds the value advertised in its
        //# active_connection_id_limit transport parameter, an endpoint MUST
        //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.

        Ok(())
    }

    /// Writes any RETIRE_CONNECTION_ID frames necessary to the given context
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let constraint = context.transmission_constraint();

        for mut id_info in self
            .registered_ids
            .iter_mut()
            .filter(|id_info| id_info.status.can_transmit(constraint))
        {
            if let Some(packet_number) = context.write_frame(&frame::RetireConnectionID {
                sequence_number: Default::default(),
            }) {
                id_info.status = PendingAcknowledgement(packet_number);
            }
        }
    }

    /// Removes connection IDs that were pending acknowledgement
    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A) {
        self.registered_ids.retain(|id_info| {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                // Don't retain the ID that was acknowledged
                !ack_set.contains(packet_number)
            } else {
                // Retain IDs that weren't PendingAcknowledgement
                true
            }
        });
    }

    /// Sets the retransmit flag to true for connection IDs pending acknowledgement with a lost
    /// packet number
    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        for mut id_info in self.registered_ids.iter_mut() {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    id_info.status = PendingRetirement(true);
                }
            }
        }
    }
}

impl transmission::interest::Provider for PeerIdRegistry {
    fn transmission_interest(&self) -> Interest {
        let has_ids_pending_retirement_again = self
            .registered_ids
            .iter()
            .any(|id_info| id_info.status == PendingRetirement(true));

        if has_ids_pending_retirement_again {
            return transmission::Interest::LostData;
        }

        let has_ids_pending_retirement = self
            .registered_ids
            .iter()
            .any(|id_info| id_info.status == PendingRetirement(false));

        if has_ids_pending_retirement {
            transmission::Interest::NewData
        } else {
            transmission::Interest::None
        }
    }
}
