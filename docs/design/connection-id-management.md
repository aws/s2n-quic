# s2n-QUIC Connection ID Management

## Background & Motivation

One benefit of QUIC over other transport protocols is the use of connection IDs for identifying connections between
endpoints, rather than IP address and port. Since IP address and port necessarily change when a client switches network
paths (by switching from Wi-Fi to 5G for example), protocols relying on IP and port for identifying connections must
re-establish existing connections in such a case. QUIC, on the other hand, can migrate connections across paths without
losing continuity. The [QUIC Transport RFC](https://tools.ietf.org/id/draft-ietf-quic-transport-32.html) specifies
mechanisms for QUIC connections to notify peers of new connection IDs as well as retire connection IDs that will no
longer be used. The RFC also specifies when these mechanisms must be used for the purpose of protecting privacy;
primarily during connection migration. A potential need for greater privacy protection may warrant additional rotation
of connection IDs beyond that which is specified in the RFC. This document details the issues surrounding connection ID
management in s2n-QUIC and recommends approaches that balance complexity, privacy, and flexibility.

## Requirements

The QUIC Transport RFC details the requirements for generating, issuing, and retiring connection IDs. These requirements
are listed in Appendix A: RFC Requirements. At a high level, s2n-QUIC must be able to:

* Send NEW_CONNECTION_ID frames, indicating the endpoint has generated a new connection ID that a peer can use for
  routing packets to it
* Receive NEW_CONNECTION_ID frames, indicating that packets may be routed to the peer using that connection ID
* Send RETIRE_CONNECTION_ID frames, indicating the endpoint no longer will use a connection ID that the peer had
  previously communicated
* Receive RETIRE_CONNECTION_ID frames, indicating the peer will no longer use a connection ID and a new connection ID
  should be issued
* Manage the collection of connection IDs received from a peer and decide which connection ID to use when sending
  packets to a peer, including switching to an unused connection ID when the peer migrates paths
* Generate stateless reset tokens that may be used to reset connections when packets are received after a crash
* Allow an s2n-QUIC endpoint to specify a preferred address for a client to send traffic to after completing the
  handshake
* Support a peer using zero-length connection IDs, indicating the peer does not require a connection ID to route packets
  to them

From the launch customer of s2n-QUIC, Cloudfront, s2n-QUIC must be able to:

* Respect the validity duration Cloudfront specifies for their connection IDs by ceasing use of a connection ID prior to
  it expiring and requesting Cloudfront generate a new one. CloudFront has a continually rotating connection ID key,
  which prevents an attacker from generating an unbounded number of connection IDs and executing a connection ID
  exhausting denial of service attack on their hosts. By limiting how long a connection ID can be used for, they can
  block traffic from such an attack from penetrating far into the network.

Beyond these requirements, there is optional behavior surrounding connection IDs that we may choose to implement to
enhance the security/privacy characteristics of s2n-QUIC. These optional features include:

* Rotating connection IDs after the combined cryptographic and transport handshake has completed
* Rotating connection IDs at a regular interval

## Issues and Alternatives

From
the [QUIC Transport RFC section 5.1](https://tools.ietf.org/id/draft-ietf-quic-transport-32.html#name-connection-id):


> Each connection possesses a set of connection identifiers, or connection IDs, each of which can identify the connection. Connection IDs are independently selected by endpoints; each endpoint selects the connection IDs that its peer uses.


Given the independent selection of connection IDs by a given QUIC endpoint and its peer, there are design, security, and
privacy implications to consider across both the local connection IDs selected by the endpoint and the connection IDs
selected by the peer. While there is some overlap between the two, for the purpose of this document the issues
surrounding local connection IDs and connection IDs from the peer are handled separately below:

### **Local Connection IDs**

The issues described below pertain to the processes surrounding a given QUIC endpoint generating connection IDs and
communicating the selected connection IDs to a peer for the purpose of addressing a given connection.

**Issue: Should an s2n-QUIC endpoint issue a new connection ID immediately after completing the combined cryptographic
and transport handshake?**

Appendix B shows the unprotected data present in the header of packets sent prior to handshake completion, which QUIC
calls the “long header” as it contains additional data when compared to the “short header” used after handshake
completion. Initial packets (which carry the ClientHello and ServerHello) in particular, have no confidentiality
protection. If a connection ID used prior to handshake continues to be used post-handshake, this unprotected data could
be linked to all post-handshake packets until the connection ID is retired.

**Option 1 (Recommended): Yes**

* Reduced linkability (+): Decreased possibility of the unprotected data present in pre-handshake packets being linked
  to post-handshake packets. As the ClientHello is present in pre-handshake packets, without rotating the connection ID
  the Server Name Indication (SNI) contained within could be linked to post-handshake packets. This would allow for
  subsequent packets to be tracked or blocked by on-path elements based on the SNI.
* Middlebox confusion (-): The more frequently you rotate IDs, the more difficulty a middlebox will have to analyze the
  connections. Sending new connection IDs may make the middle believe there are more connections behind a NAT than there
  really are, and generally prevent a middlebox from accurately understanding traffic patterns. Given the new connection
  ID would be sent immediately after the handshake, there wouldn’t be much traffic sent under the old connection ID, so
  this should not be a major concern.

**Option 2: Default yes, but configurable**

* Ability to opt-out (+): Customers that cannot use this feature for some reason will have the ability to turn it off
* Additional configuration (-): Increased maintenance burden on a configuration option that may not be used

**Option 3: Default no, but configurable**

* Ability to opt-out (+): Customers that cannot use this feature for some reason will not have to do anything
* Increased linkability by default (-)
* Additional configuration (-): Increased maintenance burden on a configuration option that may not be used

**Option 4: No**

* Simple (+)
* Increased linkability (-)

The recommendation is Option 1, issuing a new connection ID immediately after handshake for all customers. This reduces
linkability and increases privacy by default. If a customer does need to opt-out, this can be made configurable at a
later date.

**Issue: Should an s2n-QUIC endpoint issue new connection IDs at a regular time interval?**

Customers who have implemented their own connection ID generation have the ability to specify the duration each
generated connection ID is valid for:

```
pub trait Generator {
    /// Generates a connection ID with an optional validity duration
    fn generate(&mut self) -> (Id, Option<core::time::Duration>);
}
```

For customers that instead use s2n-QUIC’s default connection ID generator, should those connection IDs have a default
validity duration? With long-lived connection IDs, an on-path attacker can accumulate many valid connection IDs over
time. The attacker could attempt a denial-of-service (DoS) attack by injecting packets utilizing valid connection IDs.
These packets may be able to penetrate further into a network than a DoS attack that utilizes IP address alone.
Proactively issuing new connection IDs and retiring old ones can limit the amount of valid connection IDs an attacker
could accumulate and thus reduce the magnitude of this DoS attack. Long-lived connection IDs also allow on-path
observers to measure the amount of the data being transmitted for each connection, a potential privacy concern.

**Option 1: Yes**

* Protection from Connection ID exhaustion attack (+)
* Increased privacy (+)
* Difficulty to change default validity duration (-): Once we decide on a default validity duration, changing that value
  could impact existing customers.

**Option 2: Default yes, but configurable**

* Default protection from Connection ID exhaustion attack (+)
* Increased privacy by default (+)
* Difficulty to change default validity duration (-)
* Additional configuration (-): Increased maintenance burden on a configuration option that may not be used

**Option 3 (Recommended): Default no, but configurable**

* No default protection from Connection ID exhaustion attack (-)
* No increased privacy by default (-)
* No default validity duration (+): Since it will be up to customers to supply the default validity duration, we will
  not need to determine a default value, eliminating the risk of having to change the default in the future.
* Additional configuration (-): Increased maintenance burden on a configuration option that may not be used

**Option 4: No**

* Simple (+)
* No default validity duration (+)
* No increased privacy (-): Customers who want increased privacy will need to implement their own connection ID
  generator.
* No protection from Connection ID exhaustion attack (-): Customers who want protection from this attack will need to
  implement their own connection ID generator.

The recommendation is Option 3, allowing for a default validity duration to be specified for the default connection ID
generator, but otherwise not forcing connection ID rotation. It is not clear there is a single default value that would
be appropriate across customers, so this option lets the customer easily specify a value that works for them without
locking s2n-QUIC into a default value. To implement this, a `with_valid_duration` method would be added to the Builder
for the default connection ID generator:

```
pub fn with_valid_duration(mut self, valid_duration: core::time::Duration) -> Result<Self, c
```

**Issue: How does the customer specify the preferred address if they want it?**


> 9.6: QUIC allows servers to accept connections on one IP address and attempt to transfer these connections to a more preferred address shortly after the handshake. This is particularly useful when clients initially connect to an address shared by multiple servers but would prefer to use a unicast address to ensure connection stability.


**Option 1 (Recommended): Create a Preferred Address Provider trait**

* Consistency (+): Other optional functionality in s2n-QUIC, such as the Connection ID Generator, are defined as
  provider traits that allow a customer to implement their own custom logic.
* Flexibility (+): Gives the customer flexibility to determine how a preferred address is generated. The trait may
  include additional inputs (such as the current server address) to allow for additional custom logic.
* Increased complexity for customers (-): Customers that just want a single preferred address for all connections would
  still need to implement the Preferred Address Provider trait

**Option 2: Add preferred address to connection Config**

* Simple (+): The customer would just need to set one value to use a preferred address
* Rigid (-): Preferred address would only be allowed to be set once, which would prevent anything beyond the simplest
  use case from being allowed.

The recommendation is Option 1, defining a Preferred Address Provider trait. This is consistent with other configurable
features of s2n-QUIC and allows for customers the flexibility to implement custom logic.

**Issue: How do we generate the Stateless Reset Token associated with every connection ID?**


> 10.3: A stateless reset is provided as an option of last resort for an endpoint that does not have access to the state of a connection. A crash or outage might result in peers continuing to send data to an endpoint that is unable to properly continue the connection. An endpoint MAY send a stateless reset in response to receiving a packet that it cannot associate with an active connection. [...] To support this process, a token is sent by endpoints.


**Option 1: Generate a random Stateless Reset Token for each connection ID**

* Simple (+)
* Requires maintaining state (-): Stateless reset only exists to handle when an endpoint has lost state (such as due to
  a crash) so it is likely the stateless reset token would be lost in such a case as well

**Option 2 (Recommended): Create a Stateless Reset Token provider that the customer implements**

```
/// A generator for the Stateless reset token
pub trait StatelessResetTokenGenerator {
    /// Generates a stateless reset token based on the given connection ID
    fn generate(&mut self, connection::Id) -> (Token);
    /// Recover a stateless reset token from the given connection ID
    /// Returns None if no stateless reset token could be recovered
    fn recover(&mut self, connection::Id) -> (Option<Token>);
}
```

* Stateless (+): The stateless reset token can be generated based on the destination connection ID of the received
  packet that triggered the stateless reset.
* Zero-length connection IDs cannot be provided (-): Since the received packet must have a connection ID, zero-length
  connection IDs cannot be used.

**Option 3: Create a Stateless Reset key provider that the client implements**

This is similar to option 2, though the customer would only provide the static key and s2n-QUIC would perform the actual
generation of the Stateless Reset Token given that key. The key provided by the stateless reset key provider would be
used as follows:

> A single static key can be used across all connections to the same endpoint by generating the proof using a second 
> iteration of a preimage-resistant function that takes a static key and the connection ID chosen by the endpoint as 
> input. An endpoint could use HMAC ([RFC2104](https://tools.ietf.org/id/draft-ietf-quic-transport-32.html#RFC2104)) 
> (for example, HMAC(static_key, connection_id)) or HKDF 
> ([RFC5869](https://tools.ietf.org/id/draft-ietf-quic-transport-32.html#RFC5869)) (for example, using the static key as
> input keying material, with the connection ID as salt). The output of this function is truncated to 16 bytes to 
> produce the Stateless Reset Token for that connection.

* Stateless (+)
* Zero-length connection IDs cannot be provided (-)
* More complex (-): The process of generating the stateless reset token would be internal to s2n-QUIC. Customers would
  not be able to provide their own logic for generating the token.

```
/// A provider for the Stateless reset key
pub trait StatelessResetKeyProvider {
    /// Retrieve the static key used for stateless reset token generation
    fn retrieve(&mut self) -> (Key);
}
```

**Option 4: Require the connection ID generator to also provide the stateless reset token**

```
/// A generator for a connection ID format
pub trait Generator {
    /// Generates a connection ID with an optional validity duration and optional
    /// stateless reset token
    fn generate(&mut self) -> (Id, Option<core::time::Duration>, Option<Token>);
    /// Recover a stateless reset token from the given connection ID
    /// Returns None if no stateless reset token could be recovered
    fn recover(&mut self, connection::Id) -> (Option<Token>);
}
```

* Stateless (+)
* Zero-length connection IDs cannot be provided (-)
* Less providers to implement (+): Since the stateless reset token is by nature tied to the connection ID, it is likely
  that generating the token will utilize the same key used to generate the connection ID. Combining the generation of
  connection ID and stateless reset token in one provider may be simpler to implement for some customers
* Increased possibility to implement incorrectly (-): An incorrectly implemented stateless reset token gives an attacker
  the ability to terminate connections at will. By including the stateless reset token in the connection ID generator,
  customers may feel obligated to implement it even though it is optional. This may lead to incorrect implementations.

The recommendation is Option 2, creating a separate StatelessResetTokenGenerator that customers will need to implement
if they want stateless reset token functionality. If a customer choses to not implement this provider, s2n-QUIC will
generate a random stateless reset token and will not send stateless resets when receiving packets with unknown
connection IDs.

**Issue: Should we support using zero-length connection IDs locally?**


> 5.1: A zero-length connection ID can be used when a connection ID is not needed to route to the correct endpoint.


**Option 1: Yes**

    * Smaller packet header (+): For use cases such as SALTY that have very short lived connections, the slightly higher packet header size of connection IDs may not be worth it.
    * Does not allow for connection migration (-): “An endpoint SHOULD NOT initiate migration with a peer that has requested a zero-length connection ID, because traffic over the new path might be trivially linkable to traffic over the old one.”
    * Does not for NAT rebinding and client port reuse (-): “Multiplexing connections on the same local IP address and port while using zero-length connection IDs will cause failures in the presence of peer connection migration, NAT rebinding, and client port reuse. An endpoint MUST NOT use the same IP address and port for multiple connections with zero-length connection IDs, unless it is certain that those protocol features are not in use.”
    * Complicates generating a stateless reset token (-): A stateless reset token generation strategy that relies on connection ID is not compatible with zero-length connection IDs.

**Option 2 (Recommended): No**

    * Simple (+): Not using zero-length connection IDs eliminates special handling logic 
    * No current need (+): While SALTY may eventually want this, there is not a current need for it

The recommendation is to not use zero-length connection IDs locally, as they prevent some of the features of QUIC from
being used and they add additional complexity without having a current use case for their use.

**Issue: How many connection IDs should we give to the peer for their pool?**

The QUIC Transport RFC states that “an endpoint SHOULD ensure that its peer has a sufficient number of available and
unused connection IDs”, but does not provide specific guidance on what is sufficient. Peers may advertise they are
willing to manage hundreds or thousands of connection IDs, but there is no requirement that an s2n-QUIC endpoint
actually issue connection IDs up to the peers limit. The RFC calls out that “An endpoint MAY also limit the issuance of
connection IDs to reduce the amount of per-path state it maintains, such as path validation status, as its peer might
interact with it over as many paths as there are issued connection IDs.” On the other hand, issuing too few connection
IDs prohibits the peer from probing new paths or initiating connection migration.

**Option 1: One**

* Minimal state to manage (+)
* Prevents peer from probing paths/connection migration (-): To reduce linkability, s2n-QUIC clients are required to
  consume unused connection IDs when probing for new paths or initiating connection migration. If an s2n-QUIC endpoint
  does not supply more than one connection ID to a peer, it will not be able to perform these functions.

**Option 2: Two**

* Minimal state to manage (+)
* Allows peer to probe one path and migrate connections (+)
* Limits probing of peer (-): For a peer that is moving between unreliable networks, they may want to probe several
  different paths to determine a valid path. If the peer only has one spare connection ID, they will be limited to
  probing only a single path.

**Option 3 (Recommended): Three+**

* Increased state to manage (-)
* Allows peer to probe multiple paths and migrate connections (+)

**Option 4: Configurable**

* Flexible (+)
* Additional configuration (-): Increased maintenance burden on a configuration option that may not be used

The recommendation is Option 3, a default of at least 3. Giving at least 3 connection IDs to a peer allows the peer to
perform probing across multiple paths, which is useful in unreliable networks. Peers may always retire connection IDs
they are not using and s2n-QUIC will issue new connection IDs in response. Eventually this may need to be configurable,
but that should not be necessary until s2n-QUIC has a more diverse set of customer use cases.

### **Connection IDs from the Peer**

The issues described below pertain to the processes surrounding a given QUIC endpoint receiving the connection IDs a
peer selected to address a given connection.

**Issue: What should be the default active connection ID limit?**

   The active_connection_id_limit MUST be 3.

> 18.2:  The active connection ID limit is an integer value specifying the maximum number of connection IDs from the peer that an endpoint is willing to store. This value includes the connection ID received during the handshake, that received in the preferred_address transport parameter, and those received in NEW_CONNECTION_ID frames.


**Option 1 (Recommended): Two**

* Minimal connection state (+): Each connection ID being maintained introduces additional state to track, since each may
  interact with the endpoint over any of the connection IDs that are active. Setting the active connection ID limit to
  2 (the minimum allowed) minimizes the amount of connection state being maintained.
* Prevents connection migration (-): Since migrating to a new connection requires consuming a previously unused
  connection ID, with only 2 total active connection IDs there would be none left after the connection ID received
  during the handshake and the preferred address transport parameter. This is only an issue for clients though, as
  servers do not migrate connections beyond the option preferred address migration.

**Option 2: More than two**

* Additional connection state (-)
* Allows for connection migration (+)

**Option 3: Configurable**

* Flexible (+)
* Additional configuration (-): Increased maintenance burden on a configuration option that may not be used

The recommendation is two, which is the default active connection ID limit if no value is provided. Since s2n-QUIC is
targeting server deployments first, connection migration is not a concern. If the peer supplies the maximum of two
connection IDs, the s2n-QUIC endpoint will have exactly one unused connection ID to use during connection ID rotation.
This should be revisited as more client based customer use cases develop.

**Issue: Should an s2n-QUIC endpoint retire the initial connection ID received from the peer immediately after
completing the combined cryptographic and transport handshake?**

Similar to local connection IDs, allowing the peer to use the same connection ID pre-handshake and post-handshake allows
for the unprotected data present pre-handshake (Appendix A) to be linked to post-handshake packets. The difference with
peer connection IDs though is that an s2n-QUIC endpoint can only retire a connection ID from the peer if the endpoint
has a spare connection ID to start using. Thus, if the peer does not give the s2n-QUIC endpoint a connection ID other
than the initial connection ID, we cannot rotate connection IDs after handshake. So far, aioquic, ngtcp2, and picoquic
are the only known clients that proactively deliver new connection IDs from their client implementations.

**Option 1 (Recommended): Yes**

* Reduced linkability (+)
* Middlebox confusion (-)

**Option 2: Default yes, but configurable**

* Ability to opt-out (+)
* Additional configuration (-)

**Option 3: Default no, but configurable**

* Ability to opt-out (+)
* Increased linkability by default (-)
* Additional configuration (-)

**Option 4: No**

* Simple (+)
* Increased linkability (-)

As with local connection IDs, the recommendation is Option 1. Given the lack of support from QUIC clients though, this
feature is not critical for the initial release of s2n-QUIC.

**Issue: Should an s2n-QUIC endpoint retire existing connection IDs from the peer at a regular time interval?**

An s2n-QUIC endpoint does not control whether its peer rotates connection IDs, but it can issue Retire Connection ID
requests to the peer that should result in the peer issuing a new connection ID. As with the previous issue, it is up to
the peer whether they deliver enough connection IDs to an s2n-QUIC endpoint to make this possible. In addition, since
s2n-QUIC is initially targeting server deployments, the connection ID exhaustion attack described for local connection
IDs is unlikely on the peer, as clients would only make a limited number of connections to a given s2n-QUIC endpoint.

**Option 1: Yes**

* Increased privacy (+)
* Difficulty to change default validity duration (-): Once we decide on a default validity duration, changing that value
  could impact existing customers.

**Option 2: Default yes, but configurable**

* Increased privacy by default (+)
* Difficulty to change default validity duration (-)
* Additional configuration (-): Increased maintenance burden on a configuration option that may not be used

**Option 3: Default no, but configurable**

* No increased privacy by default (-)
* No default validity duration (+): Since it will be up to customers to supply the default validity duration, we will
  not need to determine a default value, eliminating the risk of having to change the default in the future.
* Additional configuration (-): Increased maintenance burden on a configuration option that may not be used

**Option 4 (Recommended): No**

* Simple (+)
* No default validity duration (+)
* No increased privacy (-)

The recommendation is to not support this feature at this time, as it has limited QUIC client support and would require
implementing additional logic that may not be warranted. Peers may still rotate their connection IDs at will, s2n-QUIC
will just not proactively request they do.

## Security Considerations

**Peer Denial of Service**
A misbehaving/malicious peer could cause a denial of service by sending large amounts of New Connection ID and Retire
Connection ID requests. There is some amount of overhead with each New Connection ID and Retire Connection ID request (
generating the IDs, storing/removing from a hash table), so there is presumably some rate of processing these requests
that could impact the performance of the server. Rather than handling this specifically for New Connection ID and Retire
Connection ID requests, we should have a holistic solution that protects against all such peer denial service attacks,
as recommended in section 21.8:

> While there are legitimate uses for all messages, implementations SHOULD track cost of processing relative to progress and treat excessive quantities of any non-productive packets as indicative of an attack. Endpoints MAY respond to this condition with a connection error, or by dropping packets.

## Appendix A: RFC Requirements

Extracted
from [QUIC: A UDP-Based Multiplexed and Secure Transport](https://tools.ietf.org/id/draft-ietf-quic-transport-32.html)
draft 32, sections 5.1, 9.5 and 10.3

### MUST

* Connection IDs MUST NOT contain any information that can be used by an external observer (that is, one that does not
  cooperate with the issuer) to correlate them with other connection IDs for the same connection.
* The same connection ID MUST NOT be issued more than once on the same connection.
* An endpoint MUST NOT use the same IP address and port for multiple connections with zero-length connection IDs, unless
  it is certain that those protocol features are not in use.
* The sequence number on each newly issued connection ID MUST increase by 1.
* When an endpoint issues a connection ID, it MUST accept packets that carry this connection ID for the duration of the
  connection or until its peer invalidates the connection ID via a RETIRE_CONNECTION_ID frame (Section 19.16).
* An endpoint MUST NOT provide more connection IDs than the peer's limit.
* After processing a NEW_CONNECTION_ID frame and adding and retiring active connection IDs, if the number of active
  connection IDs exceeds the value advertised in its active_connection_id_limit transport parameter, an endpoint MUST
  close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
* Upon receipt of an increased Retire Prior To field, the peer MUST stop using the corresponding connection IDs and
  retire them with RETIRE_CONNECTION_ID frames before adding the newly provided connection ID to the set of active
  connection IDs.
* An endpoint MUST NOT forget a connection ID without retiring it, though it MAY choose to treat having connection IDs
  in need of retirement that exceed this limit as a connection error of type CONNECTION_ID_LIMIT_ERROR.
* The stateless reset check comparison MUST be performed when the first packet in an incoming datagram either cannot be
  associated with a connection, or cannot be decrypted.
* An endpoint MUST NOT check for any Stateless Reset Tokens associated with connection IDs it has not used or for
  connection IDs that have been retired.
* When comparing a datagram to Stateless Reset Token values, endpoints MUST perform the comparison without leaking
  information about the value of the token.
* If the last 16 bytes of the datagram are identical in value to a Stateless Reset Token, the endpoint MUST enter the
  draining period and not send any further packets on this connection.
* The stateless reset token MUST be difficult to guess.
* An endpoint that uses this design MUST either use the same connection ID length for all connections or encode the
  length of the connection ID such that it can be recovered without state.
* This method for choosing the Stateless Reset Token means that the combination of connection ID and static key MUST NOT
  be used for another connection.
* A connection ID from a connection that is reset by revealing the Stateless Reset Token MUST NOT be reused for new
  connections at nodes that share a static key.
* The same Stateless Reset Token MUST NOT be used for multiple connection IDs.
* Endpoints are not required to compare new values against all previous values, but a duplicate value MAY be treated as
  a connection error of type PROTOCOL_VIOLATION.
* An endpoint MUST NOT reuse a connection ID when sending from more than one local address, for example when initiating
  connection migration as described in Section 9.2 or when probing a new network path as described in Section 9.1.
* An endpoint MUST NOT reuse a connection ID when sending to more than one destination address.

### SHOULD/MAY

* An endpoint SHOULD ensure that its peer has a sufficient number of available and unused connection IDs.
* An endpoint MAY send connection IDs that temporarily exceed a peer's limit if the NEW_CONNECTION_ID frame also
  requires the retirement of any excess, by including a sufficiently large value in the Retire Prior To field.
* An endpoint SHOULD supply a new connection ID when the peer retires a connection ID.
* If an endpoint provided fewer connection IDs than the peer's active_connection_id_limit, it MAY supply a new
  connection ID
* An endpoint MAY limit the total number of connection IDs issued for each connection to avoid the risk of running out
  of connection IDs; see Section 10.3.2.
* An endpoint MAY also limit the issuance of connection IDs to reduce the amount of per-path state it maintains, such as
  path validation status, as its peer might interact with it over as many paths as there are issued connection IDs.
* An endpoint that initiates migration and requires non-zero-length connection IDs SHOULD ensure that the pool of
  connection IDs available to its peer allows the peer to use a new connection ID on migration, as the peer will be
  unable to respond if the pool is exhausted.
* Endpoints SHOULD retire connection IDs when they are no longer actively using either the local or destination address
  for which the connection ID was used.
* The endpoint SHOULD continue to accept the previously issued connection IDs until they are retired by the peer.
* If the endpoint can no longer process the indicated connection IDs, it MAY close the connection.
* An endpoint SHOULD limit the number of connection IDs it has retired locally and have not yet been acknowledged.
* An endpoint SHOULD allow for sending and tracking a number of RETIRE_CONNECTION_ID frames of at least twice the
  active_connection_id limit.
* Endpoints SHOULD NOT issue updates of the Retire Prior To field before receiving RETIRE_CONNECTION_ID frames that
  retire all connection IDs indicated by the previous Retire Prior To value.
* Endpoints MAY skip check for a stateless reset if any packet from a datagram is successfully processed.
* At any time, endpoints MAY change the Destination Connection ID they transmit with to a value that has not been used
  on another path.
* Due to network changes outside the control of its peer, an endpoint might receive packets from a new source address
  with the same destination connection ID, in which case it MAY continue to use the current connection ID with the new
  remote address while still sending from the same local address.
* An endpoint SHOULD NOT initiate migration with a peer that has requested a zero-length connection ID, because traffic
  over the new path might be trivially linkable to traffic over the old one.
* Changing port number can cause a peer to reset its congestion state (see Section 9.4), so the port SHOULD only be
  changed infrequently.
* To ensure that migration is possible and packets sent on different paths cannot be correlated, endpoints SHOULD
  provide new connection IDs before peers migrate; see Section 5.1.1.

## Appendix B: Unprotected Data in Long Header Packets

```
Common Unprotected Data {
  Header Form (1) = 1,
  Fixed Bit (1) = 1,
  Long Packet Type (2) = 0,
  Version (32) = 0,
  Destination Connection ID Length (8),
  Destination Connection ID (0..2040),
  Source Connection ID Length (8),
  Source Connection ID (0..2040),
}

Initial Packet {
  Token Length (i),
  Token (..),
  Length (i),
  ClientHello / ServerHello
  
}

Version Negotiation Packet {
  Supported Version (32) ...,
}

Handshake Packet {
  Length (i),
}

Retry Packet {
  Retry Token (..),
  Retry Integrity Tag (128),
}

0-RTT Packet {
  Length (i),
}
```

## Appendix C: References and Useful Links

* [Privacy Implications of Connection Migration](https://tools.ietf.org/id/draft-ietf-quic-transport-32.html#name-privacy-implications-of-con)
* [Maturing of QUIC](https://www.fastly.com/blog/maturing-of-quic)
* [Manageability of the QUIC Transport Protocol](https://www.potaroo.net/ietf/idref/draft-ietf-quic-manageability/)
* [Cloudfront Connection ID Design](https://quip-amazon.com/doQZA5FJIb45/QUIC-Connection-ID-Format)

