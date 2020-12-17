use crate::{
    connection::peer_id_registry::{
        PeerIdRegistrationError::{ExceededActiveConnectionIdLimit, InvalidNewConnectionId},
        PeerIdStatus::{
            Active, ActivePendingNewConnectionId, PendingAcknowledgement, PendingRetirement,
        },
    },
    transmission,
    transmission::{Interest, WriteContext},
};
use s2n_quic_core::{
    ack_set::AckSet, connection, frame, frame::new_connection_id::STATELESS_RESET_TOKEN_LEN,
    packet::number::PacketNumber, transport::error::TransportError,
};
use smallvec::SmallVec;

/// The amount of ConnectionIds we can register without dynamic memory allocation
const NR_STATIC_REGISTRABLE_IDS: usize = 5;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#18.2
//# The active connection ID limit is an integer value specifying the
//# maximum number of connection IDs from the peer that an endpoint is
//# willing to store. This value includes the connection ID received
//# during the handshake, that received in the preferred_address transport
//# parameter, and those received in NEW_CONNECTION_ID frames.  The value
//# of the active_connection_id_limit parameter MUST be at least 2.
pub const ACTIVE_CONNECTION_ID_LIMIT: u8 = 3;

#[derive(Debug)]
pub struct PeerIdRegistry {
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
    stateless_reset_token: Option<[u8; STATELESS_RESET_TOKEN_LEN]>,
    status: PeerIdStatus,
}

impl PeerIdInfo {
    /// Returns true if this PeerId may be used to send packets to the peer
    fn is_active(&self) -> bool {
        match self.status {
            Active => true,
            ActivePendingNewConnectionId => true,
            _ => false,
        }
    }

    /// Returns true if the status of this ID allows for transmission
    /// based on the transmission constraint
    fn can_transmit(&self, constraint: transmission::Constraint) -> bool {
        match self.status {
            PendingRetirement(true) => constraint.can_retransmit(),
            PendingRetirement(false) => constraint.can_transmit(),
            _ => false,
        }
    }
}

/// The current status of the connection ID.
#[derive(Debug, PartialEq)]
enum PeerIdStatus {
    /// Connection IDs received in NEW_CONNECTION_ID frames start in the `Active` status.
    Active,
    /// The initial connection ID used during the handshake is active, but will be retired
    /// as soon as a NEW_CONNECTION_ID frame is received from the peer.
    ActivePendingNewConnectionId,
    /// Once a connection ID will no longer be used, it enters the `PendingRetirement` status,
    /// triggering a RETIRE_CONNECTION_ID frame to be sent. The `bool` is true if the
    /// packet that sent the RETIRE_CONNECTION_ID frame was declared lost and the connection ID
    /// has re-entered `PendingRetirement` status.
    PendingRetirement(bool),
    /// Once the RETIRE_CONNECTION_ID frame has been sent, the connection ID enters
    /// `PendingAcknowledgement` status, tracking the packet number of the packet that transmitted
    /// the retire frame. When acknowledgement of that packet is received, the connection ID is
    /// removed.
    PendingAcknowledgement(PacketNumber),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerIdRegistrationError {
    /// The NEW_CONNECTION_ID frame was invalid
    InvalidNewConnectionId,
    /// The active_connection_id_limit was exceeded
    ExceededActiveConnectionIdLimit,
}

impl From<PeerIdRegistrationError> for TransportError {
    fn from(err: PeerIdRegistrationError) -> Self {
        match err {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
            //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
            //# previously issued connection ID with a different Stateless Reset
            //# Token or a different sequence number, or if a sequence number is used
            //# for different connection IDs, the endpoint MAY treat that receipt as
            //# a connection error of type PROTOCOL_VIOLATION.
            PeerIdRegistrationError::InvalidNewConnectionId => TransportError::PROTOCOL_VIOLATION,
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
            //# After processing a NEW_CONNECTION_ID frame and
            //# adding and retiring active connection IDs, if the number of active
            //# connection IDs exceeds the value advertised in its
            //# active_connection_id_limit transport parameter, an endpoint MUST
            //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
            PeerIdRegistrationError::ExceededActiveConnectionIdLimit => {
                TransportError::CONNECTION_ID_LIMIT_ERROR
            }
        }
    }
}

#[allow(dead_code)]
impl PeerIdRegistry {
    pub fn new(
        initial_connection_id: &connection::PeerId,
        stateless_reset_token: Option<[u8; STATELESS_RESET_TOKEN_LEN]>,
    ) -> Self {
        let mut registry = Self {
            registered_ids: SmallVec::new(),
            retire_prior_to: 0,
        };
        registry.registered_ids.push(PeerIdInfo {
            id: *initial_connection_id,
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
            //# The sequence number of the initial connection ID is 0.
            sequence_number: 0,
            stateless_reset_token,
            // Start the initial PeerId in ActivePendingNewConnectionId so the ID is
            // rotated as soon as the handshake completes and the peer sends a new
            // connection ID
            status: PeerIdStatus::ActivePendingNewConnectionId,
        });

        registry
    }

    pub fn on_new_connection_id(
        &mut self,
        id: &connection::PeerId,
        sequence_number: u32,
        retire_prior_to: u32,
        stateless_reset_token: [u8; STATELESS_RESET_TOKEN_LEN],
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
                && (id_info.stateless_reset_token != Some(stateless_reset_token)
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
            stateless_reset_token: Some(stateless_reset_token),
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

        // TODO combine with iteration above
        if let Some(id_info) = self
            .registered_ids
            .iter_mut()
            .find(|id_info| id_info.status == ActivePendingNewConnectionId)
        {
            id_info.status = PendingRetirement(false);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //# After processing a NEW_CONNECTION_ID frame and
        //# adding and retiring active connection IDs, if the number of active
        //# connection IDs exceeds the value advertised in its
        //# active_connection_id_limit transport parameter, an endpoint MUST
        //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
        let active_id_count = self
            .registered_ids
            .iter()
            .filter(|id_info| id_info.is_active())
            .count();
        if active_id_count > ACTIVE_CONNECTION_ID_LIMIT as usize {
            return Err(ExceededActiveConnectionIdLimit);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
        //# An endpoint SHOULD limit the number of connection IDs it has retired
        //# locally and have not yet been acknowledged. An endpoint SHOULD allow
        //# for sending and tracking a number of RETIRE_CONNECTION_ID frames of
        //# at least twice the active_connection_id limit.  An endpoint MUST NOT
        //# forget a connection ID without retiring it, though it MAY choose to
        //# treat having connection IDs in need of retirement that exceed this
        //# limit as a connection error of type CONNECTION_ID_LIMIT_ERROR.
        let retired_id_count = self.registered_ids.len() - active_id_count;
        if retired_id_count > ACTIVE_CONNECTION_ID_LIMIT as usize * 2 {
            return Err(ExceededActiveConnectionIdLimit);
        }

        Ok(())
    }

    /// Writes any RETIRE_CONNECTION_ID frames necessary to the given context
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let constraint = context.transmission_constraint();

        for mut id_info in self
            .registered_ids
            .iter_mut()
            .filter(|id_info| id_info.can_transmit(constraint))
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

    /// Returns true if the given `connection::PeerId` is currently active
    pub fn is_active(&self, id: &connection::PeerId) -> bool {
        self.registered_ids
            .iter()
            .find(|id_info| id_info.id == *id)
            .map_or(false, |id_info| id_info.is_active())
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
