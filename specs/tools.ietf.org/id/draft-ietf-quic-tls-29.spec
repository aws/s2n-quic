



QUIC                                                     M. Thomson, Ed.
Internet-Draft                                                   Mozilla
Intended status: Standards Track                          S. Turner, Ed.
Expires: 11 December 2020                                          sn3rd
                                                             9 June 2020


                        Using TLS to Secure QUIC
                         draft-ietf-quic-tls-29

Abstract

   This document describes how Transport Layer Security (TLS) is used to
   secure QUIC.

Note to Readers

   Discussion of this draft takes place on the QUIC working group
   mailing list (quic@ietf.org (mailto:quic@ietf.org)), which is
   archived at https://mailarchive.ietf.org/arch/
   search/?email_list=quic.

   Working Group information can be found at https://github.com/quicwg;
   source code and issues list for this draft can be found at
   https://github.com/quicwg/base-drafts/labels/-tls.

Status of This Memo

   This Internet-Draft is submitted in full conformance with the
   provisions of BCP 78 and BCP 79.

   Internet-Drafts are working documents of the Internet Engineering
   Task Force (IETF).  Note that other groups may also distribute
   working documents as Internet-Drafts.  The list of current Internet-
   Drafts is at https://datatracker.ietf.org/drafts/current/.

   Internet-Drafts are draft documents valid for a maximum of six months
   and may be updated, replaced, or obsoleted by other documents at any
   time.  It is inappropriate to use Internet-Drafts as reference
   material or to cite them other than as "work in progress."

   This Internet-Draft will expire on 11 December 2020.

Copyright Notice

   Copyright (c) 2020 IETF Trust and the persons identified as the
   document authors.  All rights reserved.




Thomson & Turner        Expires 11 December 2020                [Page 1]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   This document is subject to BCP 78 and the IETF Trust's Legal
   Provisions Relating to IETF Documents (https://trustee.ietf.org/
   license-info) in effect on the date of publication of this document.
   Please review these documents carefully, as they describe your rights
   and restrictions with respect to this document.  Code Components
   extracted from this document must include Simplified BSD License text
   as described in Section 4.e of the Trust Legal Provisions and are
   provided without warranty as described in the Simplified BSD License.

Table of Contents

   1.  Introduction  . . . . . . . . . . . . . . . . . . . . . . . .   4
   2.  Notational Conventions  . . . . . . . . . . . . . . . . . . .   4
     2.1.  TLS Overview  . . . . . . . . . . . . . . . . . . . . . .   5
   3.  Protocol Overview . . . . . . . . . . . . . . . . . . . . . .   7
   4.  Carrying TLS Messages . . . . . . . . . . . . . . . . . . . .   8
     4.1.  Interface to TLS  . . . . . . . . . . . . . . . . . . . .  10
       4.1.1.  Handshake Complete  . . . . . . . . . . . . . . . . .  10
       4.1.2.  Handshake Confirmed . . . . . . . . . . . . . . . . .  11
       4.1.3.  Sending and Receiving Handshake Messages  . . . . . .  11
       4.1.4.  Encryption Level Changes  . . . . . . . . . . . . . .  13
       4.1.5.  TLS Interface Summary . . . . . . . . . . . . . . . .  14
     4.2.  TLS Version . . . . . . . . . . . . . . . . . . . . . . .  15
     4.3.  ClientHello Size  . . . . . . . . . . . . . . . . . . . .  15
     4.4.  Peer Authentication . . . . . . . . . . . . . . . . . . .  16
     4.5.  Session Resumption  . . . . . . . . . . . . . . . . . . .  17
     4.6.  Enabling 0-RTT  . . . . . . . . . . . . . . . . . . . . .  17
     4.7.  Accepting and Rejecting 0-RTT . . . . . . . . . . . . . .  18
     4.8.  Validating 0-RTT Configuration  . . . . . . . . . . . . .  18
     4.9.  HelloRetryRequest . . . . . . . . . . . . . . . . . . . .  19
     4.10. TLS Errors  . . . . . . . . . . . . . . . . . . . . . . .  19
     4.11. Discarding Unused Keys  . . . . . . . . . . . . . . . . .  19
       4.11.1.  Discarding Initial Keys  . . . . . . . . . . . . . .  20
       4.11.2.  Discarding Handshake Keys  . . . . . . . . . . . . .  20
       4.11.3.  Discarding 0-RTT Keys  . . . . . . . . . . . . . . .  20
   5.  Packet Protection . . . . . . . . . . . . . . . . . . . . . .  21
     5.1.  Packet Protection Keys  . . . . . . . . . . . . . . . . .  21
     5.2.  Initial Secrets . . . . . . . . . . . . . . . . . . . . .  21
     5.3.  AEAD Usage  . . . . . . . . . . . . . . . . . . . . . . .  23
     5.4.  Header Protection . . . . . . . . . . . . . . . . . . . .  24
       5.4.1.  Header Protection Application . . . . . . . . . . . .  24
       5.4.2.  Header Protection Sample  . . . . . . . . . . . . . .  26
       5.4.3.  AES-Based Header Protection . . . . . . . . . . . . .  28
       5.4.4.  ChaCha20-Based Header Protection  . . . . . . . . . .  28
     5.5.  Receiving Protected Packets . . . . . . . . . . . . . . .  28
     5.6.  Use of 0-RTT Keys . . . . . . . . . . . . . . . . . . . .  29
     5.7.  Receiving Out-of-Order Protected Frames . . . . . . . . .  29
     5.8.  Retry Packet Integrity  . . . . . . . . . . . . . . . . .  30



Thomson & Turner        Expires 11 December 2020                [Page 2]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   6.  Key Update  . . . . . . . . . . . . . . . . . . . . . . . . .  32
     6.1.  Initiating a Key Update . . . . . . . . . . . . . . . . .  33
     6.2.  Responding to a Key Update  . . . . . . . . . . . . . . .  34
     6.3.  Timing of Receive Key Generation  . . . . . . . . . . . .  34
     6.4.  Sending with Updated Keys . . . . . . . . . . . . . . . .  35
     6.5.  Receiving with Different Keys . . . . . . . . . . . . . .  35
     6.6.  Minimum Key Update Frequency  . . . . . . . . . . . . . .  36
     6.7.  Key Update Error Code . . . . . . . . . . . . . . . . . .  37
   7.  Security of Initial Messages  . . . . . . . . . . . . . . . .  38
   8.  QUIC-Specific Adjustments to the TLS Handshake  . . . . . . .  38
     8.1.  Protocol Negotiation  . . . . . . . . . . . . . . . . . .  38
     8.2.  QUIC Transport Parameters Extension . . . . . . . . . . .  39
     8.3.  Removing the EndOfEarlyData Message . . . . . . . . . . .  39
     8.4.  Prohibit TLS Middlebox Compatibility Mode . . . . . . . .  40
   9.  Security Considerations . . . . . . . . . . . . . . . . . . .  40
     9.1.  Session Linkability . . . . . . . . . . . . . . . . . . .  40
     9.2.  Replay Attacks with 0-RTT . . . . . . . . . . . . . . . .  40
     9.3.  Packet Reflection Attack Mitigation . . . . . . . . . . .  41
     9.4.  Header Protection Analysis  . . . . . . . . . . . . . . .  42
     9.5.  Header Protection Timing Side-Channels  . . . . . . . . .  43
     9.6.  Key Diversity . . . . . . . . . . . . . . . . . . . . . .  43
   10. IANA Considerations . . . . . . . . . . . . . . . . . . . . .  44
   11. References  . . . . . . . . . . . . . . . . . . . . . . . . .  44
     11.1.  Normative References . . . . . . . . . . . . . . . . . .  44
     11.2.  Informative References . . . . . . . . . . . . . . . . .  45
   Appendix A.  Sample Packet Protection . . . . . . . . . . . . . .  46
     A.1.  Keys  . . . . . . . . . . . . . . . . . . . . . . . . . .  47
     A.2.  Client Initial  . . . . . . . . . . . . . . . . . . . . .  48
     A.3.  Server Initial  . . . . . . . . . . . . . . . . . . . . .  49
     A.4.  Retry . . . . . . . . . . . . . . . . . . . . . . . . . .  50
     A.5.  ChaCha20-Poly1305 Short Header Packet . . . . . . . . . .  50
   Appendix B.  Analysis of Limits on AEAD_AES_128_CCM Usage . . . .  52
     B.1.  Confidentiality Limits  . . . . . . . . . . . . . . . . .  53
     B.2.  Integrity Limits  . . . . . . . . . . . . . . . . . . . .  53
   Appendix C.  Change Log . . . . . . . . . . . . . . . . . . . . .  53
     C.1.  Since draft-ietf-quic-tls-28  . . . . . . . . . . . . . .  53
     C.2.  Since draft-ietf-quic-tls-27  . . . . . . . . . . . . . .  54
     C.3.  Since draft-ietf-quic-tls-26  . . . . . . . . . . . . . .  54
     C.4.  Since draft-ietf-quic-tls-25  . . . . . . . . . . . . . .  54
     C.5.  Since draft-ietf-quic-tls-24  . . . . . . . . . . . . . .  54
     C.6.  Since draft-ietf-quic-tls-23  . . . . . . . . . . . . . .  54
     C.7.  Since draft-ietf-quic-tls-22  . . . . . . . . . . . . . .  54
     C.8.  Since draft-ietf-quic-tls-21  . . . . . . . . . . . . . .  55
     C.9.  Since draft-ietf-quic-tls-20  . . . . . . . . . . . . . .  55
     C.10. Since draft-ietf-quic-tls-18  . . . . . . . . . . . . . .  55
     C.11. Since draft-ietf-quic-tls-17  . . . . . . . . . . . . . .  55
     C.12. Since draft-ietf-quic-tls-14  . . . . . . . . . . . . . .  55
     C.13. Since draft-ietf-quic-tls-13  . . . . . . . . . . . . . .  56



Thomson & Turner        Expires 11 December 2020                [Page 3]

Internet-Draft          Using TLS to Secure QUIC               June 2020


     C.14. Since draft-ietf-quic-tls-12  . . . . . . . . . . . . . .  56
     C.15. Since draft-ietf-quic-tls-11  . . . . . . . . . . . . . .  56
     C.16. Since draft-ietf-quic-tls-10  . . . . . . . . . . . . . .  56
     C.17. Since draft-ietf-quic-tls-09  . . . . . . . . . . . . . .  56
     C.18. Since draft-ietf-quic-tls-08  . . . . . . . . . . . . . .  56
     C.19. Since draft-ietf-quic-tls-07  . . . . . . . . . . . . . .  56
     C.20. Since draft-ietf-quic-tls-05  . . . . . . . . . . . . . .  57
     C.21. Since draft-ietf-quic-tls-04  . . . . . . . . . . . . . .  57
     C.22. Since draft-ietf-quic-tls-03  . . . . . . . . . . . . . .  57
     C.23. Since draft-ietf-quic-tls-02  . . . . . . . . . . . . . .  57
     C.24. Since draft-ietf-quic-tls-01  . . . . . . . . . . . . . .  57
     C.25. Since draft-ietf-quic-tls-00  . . . . . . . . . . . . . .  57
     C.26. Since draft-thomson-quic-tls-01 . . . . . . . . . . . . .  58
   Contributors  . . . . . . . . . . . . . . . . . . . . . . . . . .  58
   Authors' Addresses  . . . . . . . . . . . . . . . . . . . . . . .  59

1.  Introduction

   This document describes how QUIC [QUIC-TRANSPORT] is secured using
   TLS [TLS13].

   TLS 1.3 provides critical latency improvements for connection
   establishment over previous versions.  Absent packet loss, most new
   connections can be established and secured within a single round
   trip; on subsequent connections between the same client and server,
   the client can often send application data immediately, that is,
   using a zero round trip setup.

   This document describes how TLS acts as a security component of QUIC.

2.  Notational Conventions

   The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
   "SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and
   "OPTIONAL" in this document are to be interpreted as described in
   BCP 14 [RFC2119] [RFC8174] when, and only when, they appear in all
   capitals, as shown here.

   This document uses the terminology established in [QUIC-TRANSPORT].

   For brevity, the acronym TLS is used to refer to TLS 1.3, though a
   newer version could be used (see Section 4.2).









Thomson & Turner        Expires 11 December 2020                [Page 4]

Internet-Draft          Using TLS to Secure QUIC               June 2020


2.1.  TLS Overview

   TLS provides two endpoints with a way to establish a means of
   communication over an untrusted medium (that is, the Internet) that
   ensures that messages they exchange cannot be observed, modified, or
   forged.

   Internally, TLS is a layered protocol, with the structure shown in
   Figure 1.

             +-------------+------------+--------------+---------+
   Handshake |             |            |  Application |         |
   Layer     |  Handshake  |   Alerts   |     Data     |   ...   |
             |             |            |              |         |
             +-------------+------------+--------------+---------+
   Record    |                                                   |
   Layer     |                      Records                      |
             |                                                   |
             +---------------------------------------------------+

                            Figure 1: TLS Layers

   Each Handshake layer message (e.g., Handshake, Alerts, and
   Application Data) is carried as a series of typed TLS records by the
   Record layer.  Records are individually cryptographically protected
   and then transmitted over a reliable transport (typically TCP) which
   provides sequencing and guaranteed delivery.

   The TLS authenticated key exchange occurs between two endpoints:
   client and server.  The client initiates the exchange and the server
   responds.  If the key exchange completes successfully, both client
   and server will agree on a secret.  TLS supports both pre-shared key
   (PSK) and Diffie-Hellman over either finite fields or elliptic curves
   ((EC)DHE) key exchanges.  PSK is the basis for 0-RTT; the latter
   provides perfect forward secrecy (PFS) when the (EC)DHE keys are
   destroyed.

   After completing the TLS handshake, the client will have learned and
   authenticated an identity for the server and the server is optionally
   able to learn and authenticate an identity for the client.  TLS
   supports X.509 [RFC5280] certificate-based authentication for both
   server and client.

   The TLS key exchange is resistant to tampering by attackers and it
   produces shared secrets that cannot be controlled by either
   participating peer.

   TLS provides two basic handshake modes of interest to QUIC:



Thomson & Turner        Expires 11 December 2020                [Page 5]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   *  A full 1-RTT handshake in which the client is able to send
      Application Data after one round trip and the server immediately
      responds after receiving the first handshake message from the
      client.

   *  A 0-RTT handshake in which the client uses information it has
      previously learned about the server to send Application Data
      immediately.  This Application Data can be replayed by an attacker
      so it MUST NOT carry a self-contained trigger for any non-
      idempotent action.

   A simplified TLS handshake with 0-RTT application data is shown in
   Figure 2.

       Client                                             Server

       ClientHello
      (0-RTT Application Data)  -------->
                                                     ServerHello
                                            {EncryptedExtensions}
                                                       {Finished}
                                <--------      [Application Data]
      {Finished}                -------->

      [Application Data]        <------->      [Application Data]

       () Indicates messages protected by Early Data (0-RTT) Keys
       {} Indicates messages protected using Handshake Keys
       [] Indicates messages protected using Application Data
          (1-RTT) Keys

                     Figure 2: TLS Handshake with 0-RTT

   Figure 2 omits the EndOfEarlyData message, which is not used in QUIC;
   see Section 8.3.  Likewise, neither ChangeCipherSpec nor KeyUpdate
   messages are used by QUIC.  ChangeCipherSpec is redundant in TLS 1.3;
   see Section 8.4.  QUIC has its own key update mechanism; see
   Section 6.

   Data is protected using a number of encryption levels:

   *  Initial Keys

   *  Early Data (0-RTT) Keys

   *  Handshake Keys

   *  Application Data (1-RTT) Keys



Thomson & Turner        Expires 11 December 2020                [Page 6]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Application Data may appear only in the Early Data and Application
   Data levels.  Handshake and Alert messages may appear in any level.

   The 0-RTT handshake is only possible if the client and server have
   previously communicated.  In the 1-RTT handshake, the client is
   unable to send protected Application Data until it has received all
   of the Handshake messages sent by the server.

3.  Protocol Overview

   QUIC [QUIC-TRANSPORT] assumes responsibility for the confidentiality
   and integrity protection of packets.  For this it uses keys derived
   from a TLS handshake [TLS13], but instead of carrying TLS records
   over QUIC (as with TCP), TLS Handshake and Alert messages are carried
   directly over the QUIC transport, which takes over the
   responsibilities of the TLS record layer, as shown in Figure 3.

   +--------------+--------------+ +-------------+
   |     TLS      |     TLS      | |    QUIC     |
   |  Handshake   |    Alerts    | | Applications|
   |              |              | |  (h3, etc.) |
   +--------------+--------------+-+-------------+
   |                                             |
   |                QUIC Transport               |
   |   (streams, reliability, congestion, etc.)  |
   |                                             |
   +---------------------------------------------+
   |                                             |
   |            QUIC Packet Protection           |
   |                                             |
   +---------------------------------------------+

                           Figure 3: QUIC Layers

   QUIC also relies on TLS for authentication and negotiation of
   parameters that are critical to security and performance.

   Rather than a strict layering, these two protocols cooperate: QUIC
   uses the TLS handshake; TLS uses the reliability, ordered delivery,
   and record layer provided by QUIC.

   At a high level, there are two main interactions between the TLS and
   QUIC components:

   *  The TLS component sends and receives messages via the QUIC
      component, with QUIC providing a reliable stream abstraction to
      TLS.




Thomson & Turner        Expires 11 December 2020                [Page 7]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   *  The TLS component provides a series of updates to the QUIC
      component, including (a) new packet protection keys to install (b)
      state changes such as handshake completion, the server
      certificate, etc.

   Figure 4 shows these interactions in more detail, with the QUIC
   packet protection being called out specially.

   +------------+                               +------------+
   |            |<---- Handshake Messages ----->|            |
   |            |<- Validate 0-RTT parameters ->|            |
   |            |<--------- 0-RTT Keys ---------|            |
   |    QUIC    |<------- Handshake Keys -------|    TLS     |
   |            |<--------- 1-RTT Keys ---------|            |
   |            |<------- Handshake Done -------|            |
   +------------+                               +------------+
    |         ^
    | Protect | Protected
    v         | Packet
   +------------+
   |   QUIC     |
   |  Packet    |
   | Protection |
   +------------+

                    Figure 4: QUIC and TLS Interactions

   Unlike TLS over TCP, QUIC applications which want to send data do not
   send it through TLS "application_data" records.  Rather, they send it
   as QUIC STREAM frames or other frame types which are then carried in
   QUIC packets.

4.  Carrying TLS Messages

   QUIC carries TLS handshake data in CRYPTO frames, each of which
   consists of a contiguous block of handshake data identified by an
   offset and length.  Those frames are packaged into QUIC packets and
   encrypted under the current TLS encryption level.  As with TLS over
   TCP, once TLS handshake data has been delivered to QUIC, it is QUIC's
   responsibility to deliver it reliably.  Each chunk of data that is
   produced by TLS is associated with the set of keys that TLS is
   currently using.  If QUIC needs to retransmit that data, it MUST use
   the same keys even if TLS has already updated to newer keys.








Thomson & Turner        Expires 11 December 2020                [Page 8]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   One important difference between TLS records (used with TCP) and QUIC
   CRYPTO frames is that in QUIC multiple frames may appear in the same
   QUIC packet as long as they are associated with the same packet
   number space.  For instance, an endpoint can bundle a Handshake
   message and an ACK for some Handshake data into the same packet.

   Some frames are prohibited in different packet number spaces.  The
   rules here generalize those of TLS, in that frames associated with
   establishing the connection can usually appear in packets in any
   packet number space, whereas those associated with transferring data
   can only appear in the application data packet number space:

   *  PADDING, PING, and CRYPTO frames MAY appear in any packet number
      space.

   *  CONNECTION_CLOSE frames signaling errors at the QUIC layer (type
      0x1c) MAY appear in any packet number space.  CONNECTION_CLOSE
      frames signaling application errors (type 0x1d) MUST only appear
      in the application data packet number space.

   *  ACK frames MAY appear in any packet number space, but can only
      acknowledge packets which appeared in that packet number space.
      However, as noted below, 0-RTT packets cannot contain ACK frames.

   *  All other frame types MUST only be sent in the application data
      packet number space.

   Note that it is not possible to send the following frames in 0-RTT
   packets for various reasons: ACK, CRYPTO, HANDSHAKE_DONE, NEW_TOKEN,
   PATH_RESPONSE, and RETIRE_CONNECTION_ID.  A server MAY treat receipt
   of these frames in 0-RTT packets as a connection error of type
   PROTOCOL_VIOLATION.

   Because packets could be reordered on the wire, QUIC uses the packet
   type to indicate which keys were used to protect a given packet, as
   shown in Table 1.  When packets of different types need to be sent,
   endpoints SHOULD use coalesced packets to send them in the same UDP
   datagram.













Thomson & Turner        Expires 11 December 2020                [Page 9]

Internet-Draft          Using TLS to Secure QUIC               June 2020


       +---------------------+-----------------+------------------+
       | Packet Type         | Encryption Keys | PN Space         |
       +=====================+=================+==================+
       | Initial             | Initial secrets | Initial          |
       +---------------------+-----------------+------------------+
       | 0-RTT Protected     | 0-RTT           | Application data |
       +---------------------+-----------------+------------------+
       | Handshake           | Handshake       | Handshake        |
       +---------------------+-----------------+------------------+
       | Retry               | Retry           | N/A              |
       +---------------------+-----------------+------------------+
       | Version Negotiation | N/A             | N/A              |
       +---------------------+-----------------+------------------+
       | Short Header        | 1-RTT           | Application data |
       +---------------------+-----------------+------------------+

                 Table 1: Encryption Keys by Packet Type

   Section 17 of [QUIC-TRANSPORT] shows how packets at the various
   encryption levels fit into the handshake process.

4.1.  Interface to TLS

   As shown in Figure 4, the interface from QUIC to TLS consists of four
   primary functions:

   *  Sending and receiving handshake messages

   *  Processing stored transport and application state from a resumed
      session and determining if it is valid to accept early data

   *  Rekeying (both transmit and receive)

   *  Handshake state updates

   Additional functions might be needed to configure TLS.

4.1.1.  Handshake Complete

   In this document, the TLS handshake is considered complete when the
   TLS stack has reported that the handshake is complete.  This happens
   when the TLS stack has both sent a Finished message and verified the
   peer's Finished message.  Verifying the peer's Finished provides the
   endpoints with an assurance that previous handshake messages have not
   been modified.  Note that the handshake does not complete at both
   endpoints simultaneously.  Consequently, any requirement that is
   based on the completion of the handshake depends on the perspective
   of the endpoint in question.



Thomson & Turner        Expires 11 December 2020               [Page 10]

Internet-Draft          Using TLS to Secure QUIC               June 2020


4.1.2.  Handshake Confirmed

   In this document, the TLS handshake is considered confirmed at the
   server when the handshake completes.  At the client, the handshake is
   considered confirmed when a HANDSHAKE_DONE frame is received.

   A client MAY consider the handshake to be confirmed when it receives
   an acknowledgement for a 1-RTT packet.  This can be implemented by
   recording the lowest packet number sent with 1-RTT keys, and
   comparing it to the Largest Acknowledged field in any received 1-RTT
   ACK frame: once the latter is greater than or equal to the former,
   the handshake is confirmed.

4.1.3.  Sending and Receiving Handshake Messages

   In order to drive the handshake, TLS depends on being able to send
   and receive handshake messages.  There are two basic functions on
   this interface: one where QUIC requests handshake messages and one
   where QUIC provides handshake packets.

   Before starting the handshake QUIC provides TLS with the transport
   parameters (see Section 8.2) that it wishes to carry.

   A QUIC client starts TLS by requesting TLS handshake bytes from TLS.
   The client acquires handshake bytes before sending its first packet.
   A QUIC server starts the process by providing TLS with the client's
   handshake bytes.

   At any time, the TLS stack at an endpoint will have a current sending
   encryption level and receiving encryption level.  Encryption levels
   determine the packet type and keys that are used for protecting data.

   Each encryption level is associated with a different sequence of
   bytes, which is reliably transmitted to the peer in CRYPTO frames.
   When TLS provides handshake bytes to be sent, they are appended to
   the current flow.  Any packet that includes the CRYPTO frame is
   protected using keys from the corresponding encryption level.  Four
   encryption levels are used, producing keys for Initial, 0-RTT,
   Handshake, and 1-RTT packets.  CRYPTO frames are carried in just
   three of these levels, omitting the 0-RTT level.  These four levels
   correspond to three packet number spaces: Initial and Handshake
   encrypted packets use their own separate spaces; 0-RTT and 1-RTT
   packets use the application data packet number space.

   QUIC takes the unprotected content of TLS handshake records as the
   content of CRYPTO frames.  TLS record protection is not used by QUIC.
   QUIC assembles CRYPTO frames into QUIC packets, which are protected
   using QUIC packet protection.



Thomson & Turner        Expires 11 December 2020               [Page 11]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   QUIC is only capable of conveying TLS handshake records in CRYPTO
   frames.  TLS alerts are turned into QUIC CONNECTION_CLOSE error
   codes; see Section 4.10.  TLS application data and other message
   types cannot be carried by QUIC at any encryption level and is an
   error if they are received from the TLS stack.

   When an endpoint receives a QUIC packet containing a CRYPTO frame
   from the network, it proceeds as follows:

   *  If the packet was in the TLS receiving encryption level, sequence
      the data into the input flow as usual.  As with STREAM frames, the
      offset is used to find the proper location in the data sequence.
      If the result of this process is that new data is available, then
      it is delivered to TLS in order.

   *  If the packet is from a previously installed encryption level, it
      MUST NOT contain data which extends past the end of previously
      received data in that flow.  Implementations MUST treat any
      violations of this requirement as a connection error of type
      PROTOCOL_VIOLATION.

   *  If the packet is from a new encryption level, it is saved for
      later processing by TLS.  Once TLS moves to receiving from this
      encryption level, saved data can be provided.  When providing data
      from any new encryption level to TLS, if there is data from a
      previous encryption level that TLS has not consumed, this MUST be
      treated as a connection error of type PROTOCOL_VIOLATION.

   Each time that TLS is provided with new data, new handshake bytes are
   requested from TLS.  TLS might not provide any bytes if the handshake
   messages it has received are incomplete or it has no data to send.

   Once the TLS handshake is complete, this is indicated to QUIC along
   with any final handshake bytes that TLS needs to send.  TLS also
   provides QUIC with the transport parameters that the peer advertised
   during the handshake.

   Once the handshake is complete, TLS becomes passive.  TLS can still
   receive data from its peer and respond in kind, but it will not need
   to send more data unless specifically requested - either by an
   application or QUIC.  One reason to send data is that the server
   might wish to provide additional or updated session tickets to a
   client.

   When the handshake is complete, QUIC only needs to provide TLS with
   any data that arrives in CRYPTO streams.  In the same way that is
   done during the handshake, new data is requested from TLS after
   providing received data.



Thomson & Turner        Expires 11 December 2020               [Page 12]

Internet-Draft          Using TLS to Secure QUIC               June 2020


4.1.4.  Encryption Level Changes

   As keys for new encryption levels become available, TLS provides QUIC
   with those keys.  Separately, as keys at a given encryption level
   become available to TLS, TLS indicates to QUIC that reading or
   writing keys at that encryption level are available.  These events
   are not asynchronous; they always occur immediately after TLS is
   provided with new handshake bytes, or after TLS produces handshake
   bytes.

   TLS provides QUIC with three items as a new encryption level becomes
   available:

   *  A secret

   *  An Authenticated Encryption with Associated Data (AEAD) function

   *  A Key Derivation Function (KDF)

   These values are based on the values that TLS negotiates and are used
   by QUIC to generate packet and header protection keys (see Section 5
   and Section 5.4).

   If 0-RTT is possible, it is ready after the client sends a TLS
   ClientHello message or the server receives that message.  After
   providing a QUIC client with the first handshake bytes, the TLS stack
   might signal the change to 0-RTT keys.  On the server, after
   receiving handshake bytes that contain a ClientHello message, a TLS
   server might signal that 0-RTT keys are available.

   Although TLS only uses one encryption level at a time, QUIC may use
   more than one level.  For instance, after sending its Finished
   message (using a CRYPTO frame at the Handshake encryption level) an
   endpoint can send STREAM data (in 1-RTT encryption).  If the Finished
   message is lost, the endpoint uses the Handshake encryption level to
   retransmit the lost message.  Reordering or loss of packets can mean
   that QUIC will need to handle packets at multiple encryption levels.
   During the handshake, this means potentially handling packets at
   higher and lower encryption levels than the current encryption level
   used by TLS.

   In particular, server implementations need to be able to read packets
   at the Handshake encryption level at the same time as the 0-RTT
   encryption level.  A client could interleave ACK frames that are
   protected with Handshake keys with 0-RTT data and the server needs to
   process those acknowledgments in order to detect lost Handshake
   packets.




Thomson & Turner        Expires 11 December 2020               [Page 13]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   QUIC also needs access to keys that might not ordinarily be available
   to a TLS implementation.  For instance, a client might need to
   acknowledge Handshake packets before it is ready to send CRYPTO
   frames at that encryption level.  TLS therefore needs to provide keys
   to QUIC before it might produce them for its own use.

4.1.5.  TLS Interface Summary

   Figure 5 summarizes the exchange between QUIC and TLS for both client
   and server.  Each arrow is tagged with the encryption level used for
   that transmission.

   Client                                                    Server

   Get Handshake
                        Initial ------------->
                                                 Handshake Received
   Install tx 0-RTT Keys
                        0-RTT --------------->
                                                      Get Handshake
                        <------------- Initial
   Handshake Received
   Install Handshake keys
                                              Install rx 0-RTT keys
                                             Install Handshake keys
                                                      Get Handshake
                        <----------- Handshake
   Handshake Received
                                              Install tx 1-RTT keys
                        <--------------- 1-RTT
   Get Handshake
   Handshake Complete
                        Handshake ----------->
                                                 Handshake Received
                                              Install rx 1-RTT keys
                                                 Handshake Complete
   Install 1-RTT keys
                        1-RTT --------------->
                                                      Get Handshake
                        <--------------- 1-RTT
   Handshake Received

             Figure 5: Interaction Summary between QUIC and TLS








Thomson & Turner        Expires 11 December 2020               [Page 14]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Figure 5 shows the multiple packets that form a single "flight" of
   messages being processed individually, to show what incoming messages
   trigger different actions.  New handshake messages are requested
   after all incoming packets have been processed.  This process might
   vary depending on how QUIC implementations and the packets they
   receive are structured.

4.2.  TLS Version

   This document describes how TLS 1.3 [TLS13] is used with QUIC.

   In practice, the TLS handshake will negotiate a version of TLS to
   use.  This could result in a newer version of TLS than 1.3 being
   negotiated if both endpoints support that version.  This is
   acceptable provided that the features of TLS 1.3 that are used by
   QUIC are supported by the newer version.

   A badly configured TLS implementation could negotiate TLS 1.2 or
   another older version of TLS.  An endpoint MUST terminate the
   connection if a version of TLS older than 1.3 is negotiated.

4.3.  ClientHello Size

   The first Initial packet from a client contains the start or all of
   its first cryptographic handshake message, which for TLS is the
   ClientHello.  Servers might need to parse the entire ClientHello
   (e.g., to access extensions such as Server Name Identification (SNI)
   or Application Layer Protocol Negotiation (ALPN)) in order to decide
   whether to accept the new incoming QUIC connection.  If the
   ClientHello spans multiple Initial packets, such servers would need
   to buffer the first received fragments, which could consume excessive
   resources if the client's address has not yet been validated.  To
   avoid this, servers MAY use the Retry feature (see Section 8.1 of
   [QUIC-TRANSPORT]) to only buffer partial ClientHello messages from
   clients with a validated address.

   QUIC packet and framing add at least 36 bytes of overhead to the
   ClientHello message.  That overhead increases if the client chooses a
   connection ID without zero length.  Overheads also do not include the
   token or a connection ID longer than 8 bytes, both of which might be
   required if a server sends a Retry packet.










Thomson & Turner        Expires 11 December 2020               [Page 15]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   A typical TLS ClientHello can easily fit into a 1200 byte packet.
   However, in addition to the overheads added by QUIC, there are
   several variables that could cause this limit to be exceeded.  Large
   session tickets, multiple or large key shares, and long lists of
   supported ciphers, signature algorithms, versions, QUIC transport
   parameters, and other negotiable parameters and extensions could
   cause this message to grow.

   For servers, in addition to connection IDs and tokens, the size of
   TLS session tickets can have an effect on a client's ability to
   connect efficiently.  Minimizing the size of these values increases
   the probability that clients can use them and still fit their
   ClientHello message in their first Initial packet.

   The TLS implementation does not need to ensure that the ClientHello
   is sufficiently large.  QUIC PADDING frames are added to increase the
   size of the packet as necessary.

4.4.  Peer Authentication

   The requirements for authentication depend on the application
   protocol that is in use.  TLS provides server authentication and
   permits the server to request client authentication.

   A client MUST authenticate the identity of the server.  This
   typically involves verification that the identity of the server is
   included in a certificate and that the certificate is issued by a
   trusted entity (see for example [RFC2818]).

   A server MAY request that the client authenticate during the
   handshake.  A server MAY refuse a connection if the client is unable
   to authenticate when requested.  The requirements for client
   authentication vary based on application protocol and deployment.

   A server MUST NOT use post-handshake client authentication (as
   defined in Section 4.6.2 of [TLS13]), because the multiplexing
   offered by QUIC prevents clients from correlating the certificate
   request with the application-level event that triggered it (see
   [HTTP2-TLS13]).  More specifically, servers MUST NOT send post-
   handshake TLS CertificateRequest messages and clients MUST treat
   receipt of such messages as a connection error of type
   PROTOCOL_VIOLATION.









Thomson & Turner        Expires 11 December 2020               [Page 16]

Internet-Draft          Using TLS to Secure QUIC               June 2020


4.5.  Session Resumption

   QUIC can use the session resumption feature of TLS 1.3.  It does this
   by carrying NewSessionTicket messages in CRYPTO frames after the
   handshake is complete.  Session resumption is the basis of 0-RTT, but
   can be used without also enabling 0-RTT.

   Endpoints that use session resumption might need to remember some
   information about the current connection when creating a resumed
   connection.  TLS requires that some information be retained; see
   Section 4.6.1 of [TLS13].  QUIC itself does not depend on any state
   being retained when resuming a connection, unless 0-RTT is also used;
   see Section 4.6 and Section 7.4.1 of [QUIC-TRANSPORT].  Application
   protocols could depend on state that is retained between resumed
   connections.

   Clients can store any state required for resumption along with the
   session ticket.  Servers can use the session ticket to help carry
   state.

   Session resumption allows servers to link activity on the original
   connection with the resumed connection, which might be a privacy
   issue for clients.  Clients can choose not to enable resumption to
   avoid creating this correlation.  Client SHOULD NOT reuse tickets as
   that allows entities other than the server to correlate connections;
   see Section C.4 of [TLS13].

4.6.  Enabling 0-RTT

   To communicate their willingness to process 0-RTT data, servers send
   a NewSessionTicket message that contains the "early_data" extension
   with a max_early_data_size of 0xffffffff; the amount of data which
   the client can send in 0-RTT is controlled by the "initial_max_data"
   transport parameter supplied by the server.  Servers MUST NOT send
   the "early_data" extension with a max_early_data_size set to any
   value other than 0xffffffff.  A client MUST treat receipt of a
   NewSessionTicket that contains an "early_data" extension with any
   other value as a connection error of type PROTOCOL_VIOLATION.

   A client that wishes to send 0-RTT packets uses the "early_data"
   extension in the ClientHello message of a subsequent handshake (see
   Section 4.2.10 of [TLS13]).  It then sends the application data in
   0-RTT packets.








Thomson & Turner        Expires 11 December 2020               [Page 17]

Internet-Draft          Using TLS to Secure QUIC               June 2020


4.7.  Accepting and Rejecting 0-RTT

   A server accepts 0-RTT by sending an early_data extension in the
   EncryptedExtensions (see Section 4.2.10 of [TLS13]).  The server then
   processes and acknowledges the 0-RTT packets that it receives.

   A server rejects 0-RTT by sending the EncryptedExtensions without an
   early_data extension.  A server will always reject 0-RTT if it sends
   a TLS HelloRetryRequest.  When rejecting 0-RTT, a server MUST NOT
   process any 0-RTT packets, even if it could.  When 0-RTT was
   rejected, a client SHOULD treat receipt of an acknowledgement for a
   0-RTT packet as a connection error of type PROTOCOL_VIOLATION, if it
   is able to detect the condition.

   When 0-RTT is rejected, all connection characteristics that the
   client assumed might be incorrect.  This includes the choice of
   application protocol, transport parameters, and any application
   configuration.  The client therefore MUST reset the state of all
   streams, including application state bound to those streams.

   A client MAY attempt to send 0-RTT again if it receives a Retry or
   Version Negotiation packet.  These packets do not signify rejection
   of 0-RTT.

4.8.  Validating 0-RTT Configuration

   When a server receives a ClientHello with the "early_data" extension,
   it has to decide whether to accept or reject early data from the
   client.  Some of this decision is made by the TLS stack (e.g.,
   checking that the cipher suite being resumed was included in the
   ClientHello; see Section 4.2.10 of [TLS13]).  Even when the TLS stack
   has no reason to reject early data, the QUIC stack or the application
   protocol using QUIC might reject early data because the configuration
   of the transport or application associated with the resumed session
   is not compatible with the server's current configuration.

   QUIC requires additional transport state to be associated with a
   0-RTT session ticket.  One common way to implement this is using
   stateless session tickets and storing this state in the session
   ticket.  Application protocols that use QUIC might have similar
   requirements regarding associating or storing state.  This associated
   state is used for deciding whether early data must be rejected.  For
   example, HTTP/3 ([QUIC-HTTP]) settings determine how early data from
   the client is interpreted.  Other applications using QUIC could have
   different requirements for determining whether to accept or reject
   early data.





Thomson & Turner        Expires 11 December 2020               [Page 18]

Internet-Draft          Using TLS to Secure QUIC               June 2020


4.9.  HelloRetryRequest

   In TLS over TCP, the HelloRetryRequest feature (see Section 4.1.4 of
   [TLS13]) can be used to correct a client's incorrect KeyShare
   extension as well as for a stateless round-trip check.  From the
   perspective of QUIC, this just looks like additional messages carried
   in Initial packets.  Although it is in principle possible to use this
   feature for address verification in QUIC, QUIC implementations SHOULD
   instead use the Retry feature (see Section 8.1 of [QUIC-TRANSPORT]).
   HelloRetryRequest is still used to request key shares.

4.10.  TLS Errors

   If TLS experiences an error, it generates an appropriate alert as
   defined in Section 6 of [TLS13].

   A TLS alert is converted into a QUIC connection error.  The alert
   description is added to 0x100 to produce a QUIC error code from the
   range reserved for CRYPTO_ERROR.  The resulting value is sent in a
   QUIC CONNECTION_CLOSE frame of type 0x1c.

   The alert level of all TLS alerts is "fatal"; a TLS stack MUST NOT
   generate alerts at the "warning" level.

   QUIC permits the use of a generic code in place of a specific error
   code; see Section 11 of [QUIC-TRANSPORT].  For TLS alerts, this
   includes replacing any alert with a generic alert, such as
   handshake_failure (0x128 in QUIC).  Endpoints MAY use a generic error
   code to avoid possibly exposing confidential information.

4.11.  Discarding Unused Keys

   After QUIC moves to a new encryption level, packet protection keys
   for previous encryption levels can be discarded.  This occurs several
   times during the handshake, as well as when keys are updated; see
   Section 6.

   Packet protection keys are not discarded immediately when new keys
   are available.  If packets from a lower encryption level contain
   CRYPTO frames, frames that retransmit that data MUST be sent at the
   same encryption level.  Similarly, an endpoint generates
   acknowledgements for packets at the same encryption level as the
   packet being acknowledged.  Thus, it is possible that keys for a
   lower encryption level are needed for a short time after keys for a
   newer encryption level are available.






Thomson & Turner        Expires 11 December 2020               [Page 19]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   An endpoint cannot discard keys for a given encryption level unless
   it has both received and acknowledged all CRYPTO frames for that
   encryption level and when all CRYPTO frames for that encryption level
   have been acknowledged by its peer.  However, this does not guarantee
   that no further packets will need to be received or sent at that
   encryption level because a peer might not have received all the
   acknowledgements necessary to reach the same state.

   Though an endpoint might retain older keys, new data MUST be sent at
   the highest currently-available encryption level.  Only ACK frames
   and retransmissions of data in CRYPTO frames are sent at a previous
   encryption level.  These packets MAY also include PADDING frames.

4.11.1.  Discarding Initial Keys

   Packets protected with Initial secrets (Section 5.2) are not
   authenticated, meaning that an attacker could spoof packets with the
   intent to disrupt a connection.  To limit these attacks, Initial
   packet protection keys can be discarded more aggressively than other
   keys.

   The successful use of Handshake packets indicates that no more
   Initial packets need to be exchanged, as these keys can only be
   produced after receiving all CRYPTO frames from Initial packets.
   Thus, a client MUST discard Initial keys when it first sends a
   Handshake packet and a server MUST discard Initial keys when it first
   successfully processes a Handshake packet.  Endpoints MUST NOT send
   Initial packets after this point.

   This results in abandoning loss recovery state for the Initial
   encryption level and ignoring any outstanding Initial packets.

4.11.2.  Discarding Handshake Keys

   An endpoint MUST discard its handshake keys when the TLS handshake is
   confirmed (Section 4.1.2).  The server MUST send a HANDSHAKE_DONE
   frame as soon as it completes the handshake.

4.11.3.  Discarding 0-RTT Keys

   0-RTT and 1-RTT packets share the same packet number space, and
   clients do not send 0-RTT packets after sending a 1-RTT packet
   (Section 5.6).

   Therefore, a client SHOULD discard 0-RTT keys as soon as it installs
   1-RTT keys, since they have no use after that moment.





Thomson & Turner        Expires 11 December 2020               [Page 20]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Additionally, a server MAY discard 0-RTT keys as soon as it receives
   a 1-RTT packet.  However, due to packet reordering, a 0-RTT packet
   could arrive after a 1-RTT packet.  Servers MAY temporarily retain
   0-RTT keys to allow decrypting reordered packets without requiring
   their contents to be retransmitted with 1-RTT keys.  After receiving
   a 1-RTT packet, servers MUST discard 0-RTT keys within a short time;
   the RECOMMENDED time period is three times the Probe Timeout (PTO,
   see [QUIC-RECOVERY]).  A server MAY discard 0-RTT keys earlier if it
   determines that it has received all 0-RTT packets, which can be done
   by keeping track of missing packet numbers.

5.  Packet Protection

   As with TLS over TCP, QUIC protects packets with keys derived from
   the TLS handshake, using the AEAD algorithm negotiated by TLS.

5.1.  Packet Protection Keys

   QUIC derives packet protection keys in the same way that TLS derives
   record protection keys.

   Each encryption level has separate secret values for protection of
   packets sent in each direction.  These traffic secrets are derived by
   TLS (see Section 7.1 of [TLS13]) and are used by QUIC for all
   encryption levels except the Initial encryption level.  The secrets
   for the Initial encryption level are computed based on the client's
   initial Destination Connection ID, as described in Section 5.2.

   The keys used for packet protection are computed from the TLS secrets
   using the KDF provided by TLS.  In TLS 1.3, the HKDF-Expand-Label
   function described in Section 7.1 of [TLS13] is used, using the hash
   function from the negotiated cipher suite.  Other versions of TLS
   MUST provide a similar function in order to be used with QUIC.

   The current encryption level secret and the label "quic key" are
   input to the KDF to produce the AEAD key; the label "quic iv" is used
   to derive the IV; see Section 5.3.  The header protection key uses
   the "quic hp" label; see Section 5.4.  Using these labels provides
   key separation between QUIC and TLS; see Section 9.6.

   The KDF used for initial secrets is always the HKDF-Expand-Label
   function from TLS 1.3 (see Section 5.2).

5.2.  Initial Secrets

   Initial packets are protected with a secret derived from the
   Destination Connection ID field from the client's Initial packet.
   Specifically:



Thomson & Turner        Expires 11 December 2020               [Page 21]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   initial_salt = 0xafbfec289993d24c9e9786f19c6111e04390a899
   initial_secret = HKDF-Extract(initial_salt,
                                 client_dst_connection_id)

   client_initial_secret = HKDF-Expand-Label(initial_secret,
                                             "client in", "",
                                             Hash.length)
   server_initial_secret = HKDF-Expand-Label(initial_secret,
                                             "server in", "",
                                             Hash.length)

   The hash function for HKDF when deriving initial secrets and keys is
   SHA-256 [SHA].

   The connection ID used with HKDF-Expand-Label is the Destination
   Connection ID in the Initial packet sent by the client.  This will be
   a randomly-selected value unless the client creates the Initial
   packet after receiving a Retry packet, where the Destination
   Connection ID is selected by the server.

   The value of initial_salt is a 20 byte sequence shown in the figure
   in hexadecimal notation.  Future versions of QUIC SHOULD generate a
   new salt value, thus ensuring that the keys are different for each
   version of QUIC.  This prevents a middlebox that only recognizes one
   version of QUIC from seeing or modifying the contents of packets from
   future versions.

   The HKDF-Expand-Label function defined in TLS 1.3 MUST be used for
   Initial packets even where the TLS versions offered do not include
   TLS 1.3.

   The secrets used for protecting Initial packets change when a server
   sends a Retry packet to use the connection ID value selected by the
   server.  The secrets do not change when a client changes the
   Destination Connection ID it uses in response to an Initial packet
   from the server.

   Note:  The Destination Connection ID is of arbitrary length, and it
      could be zero length if the server sends a Retry packet with a
      zero-length Source Connection ID field.  In this case, the Initial
      keys provide no assurance to the client that the server received
      its packet; the client has to rely on the exchange that included
      the Retry packet for that property.

   Appendix A contains test vectors for packet encryption.






Thomson & Turner        Expires 11 December 2020               [Page 22]

Internet-Draft          Using TLS to Secure QUIC               June 2020


5.3.  AEAD Usage

   The Authentication Encryption with Associated Data (AEAD) [AEAD]
   function used for QUIC packet protection is the AEAD that is
   negotiated for use with the TLS connection.  For example, if TLS is
   using the TLS_AES_128_GCM_SHA256, the AEAD_AES_128_GCM function is
   used.

   Packets are protected prior to applying header protection
   (Section 5.4).  The unprotected packet header is part of the
   associated data (A).  When removing packet protection, an endpoint
   first removes the header protection.

   All QUIC packets other than Version Negotiation and Retry packets are
   protected with an AEAD algorithm [AEAD].  Prior to establishing a
   shared secret, packets are protected with AEAD_AES_128_GCM and a key
   derived from the Destination Connection ID in the client's first
   Initial packet (see Section 5.2).  This provides protection against
   off-path attackers and robustness against QUIC version unaware
   middleboxes, but not against on-path attackers.

   QUIC can use any of the ciphersuites defined in [TLS13] with the
   exception of TLS_AES_128_CCM_8_SHA256.  A ciphersuite MUST NOT be
   negotiated unless a header protection scheme is defined for the
   ciphersuite.  This document defines a header protection scheme for
   all ciphersuites defined in [TLS13] aside from
   TLS_AES_128_CCM_8_SHA256.  These ciphersuites have a 16-byte
   authentication tag and produce an output 16 bytes larger than their
   input.

   Note:  An endpoint MUST NOT reject a ClientHello that offers a
      ciphersuite that it does not support, or it would be impossible to
      deploy a new ciphersuite.  This also applies to
      TLS_AES_128_CCM_8_SHA256.

   The key and IV for the packet are computed as described in
   Section 5.1.  The nonce, N, is formed by combining the packet
   protection IV with the packet number.  The 62 bits of the
   reconstructed QUIC packet number in network byte order are left-
   padded with zeros to the size of the IV.  The exclusive OR of the
   padded packet number and the IV forms the AEAD nonce.

   The associated data, A, for the AEAD is the contents of the QUIC
   header, starting from the flags byte in either the short or long
   header, up to and including the unprotected packet number.

   The input plaintext, P, for the AEAD is the payload of the QUIC
   packet, as described in [QUIC-TRANSPORT].



Thomson & Turner        Expires 11 December 2020               [Page 23]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   The output ciphertext, C, of the AEAD is transmitted in place of P.

   Some AEAD functions have limits for how many packets can be encrypted
   under the same key and IV (see for example [AEBounds]).  This might
   be lower than the packet number limit.  An endpoint MUST initiate a
   key update (Section 6) prior to exceeding any limit set for the AEAD
   that is in use.

5.4.  Header Protection

   Parts of QUIC packet headers, in particular the Packet Number field,
   are protected using a key that is derived separate to the packet
   protection key and IV.  The key derived using the "quic hp" label is
   used to provide confidentiality protection for those fields that are
   not exposed to on-path elements.

   This protection applies to the least-significant bits of the first
   byte, plus the Packet Number field.  The four least-significant bits
   of the first byte are protected for packets with long headers; the
   five least significant bits of the first byte are protected for
   packets with short headers.  For both header forms, this covers the
   reserved bits and the Packet Number Length field; the Key Phase bit
   is also protected for packets with a short header.

   The same header protection key is used for the duration of the
   connection, with the value not changing after a key update (see
   Section 6).  This allows header protection to be used to protect the
   key phase.

   This process does not apply to Retry or Version Negotiation packets,
   which do not contain a protected payload or any of the fields that
   are protected by this process.

5.4.1.  Header Protection Application

   Header protection is applied after packet protection is applied (see
   Section 5.3).  The ciphertext of the packet is sampled and used as
   input to an encryption algorithm.  The algorithm used depends on the
   negotiated AEAD.

   The output of this algorithm is a 5 byte mask which is applied to the
   protected header fields using exclusive OR.  The least significant
   bits of the first byte of the packet are masked by the least
   significant bits of the first mask byte, and the packet number is
   masked with the remaining bytes.  Any unused bytes of mask that might
   result from a shorter packet number encoding are unused.





Thomson & Turner        Expires 11 December 2020               [Page 24]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Figure 6 shows a sample algorithm for applying header protection.
   Removing header protection only differs in the order in which the
   packet number length (pn_length) is determined.

   mask = header_protection(hp_key, sample)

   pn_length = (packet[0] & 0x03) + 1
   if (packet[0] & 0x80) == 0x80:
      # Long header: 4 bits masked
      packet[0] ^= mask[0] & 0x0f
   else:
      # Short header: 5 bits masked
      packet[0] ^= mask[0] & 0x1f

   # pn_offset is the start of the Packet Number field.
   packet[pn_offset:pn_offset+pn_length] ^= mask[1:1+pn_length]

                   Figure 6: Header Protection Pseudocode

   Figure 7 shows an example long header packet (Initial) and a short
   header packet.  Figure 7 shows the fields in each header that are
   covered by header protection and the portion of the protected packet
   payload that is sampled.




























Thomson & Turner        Expires 11 December 2020               [Page 25]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Initial Packet {
     Header Form (1) = 1,
     Fixed Bit (1) = 1,
     Long Packet Type (2) = 0,
     Reserved Bits (2),         # Protected
     Packet Number Length (2),  # Protected
     Version (32),
     DCID Len (8),
     Destination Connection ID (0..160),
     SCID Len (8),
     Source Connection ID (0..160),
     Token Length (i),
     Token (..),
     Length (i),
     Packet Number (8..32),     # Protected
     Protected Payload (0..24), # Skipped Part
     Protected Payload (128),   # Sampled Part
     Protected Payload (..)     # Remainder
   }

   Short Header Packet {
     Header Form (1) = 0,
     Fixed Bit (1) = 1,
     Spin Bit (1),
     Reserved Bits (2),         # Protected
     Key Phase (1),             # Protected
     Packet Number Length (2),  # Protected
     Destination Connection ID (0..160),
     Packet Number (8..32),     # Protected
     Protected Payload (0..24), # Skipped Part
     Protected Payload (128),   # Sampled Part
     Protected Payload (..),    # Remainder
   }

             Figure 7: Header Protection and Ciphertext Sample

   Before a TLS ciphersuite can be used with QUIC, a header protection
   algorithm MUST be specified for the AEAD used with that ciphersuite.
   This document defines algorithms for AEAD_AES_128_GCM,
   AEAD_AES_128_CCM, AEAD_AES_256_GCM (all AES AEADs are defined in
   [AEAD]), and AEAD_CHACHA20_POLY1305 [CHACHA].  Prior to TLS selecting
   a ciphersuite, AES header protection is used (Section 5.4.3),
   matching the AEAD_AES_128_GCM packet protection.

5.4.2.  Header Protection Sample

   The header protection algorithm uses both the header protection key
   and a sample of the ciphertext from the packet Payload field.



Thomson & Turner        Expires 11 December 2020               [Page 26]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   The same number of bytes are always sampled, but an allowance needs
   to be made for the endpoint removing protection, which will not know
   the length of the Packet Number field.  In sampling the packet
   ciphertext, the Packet Number field is assumed to be 4 bytes long
   (its maximum possible encoded length).

   An endpoint MUST discard packets that are not long enough to contain
   a complete sample.

   To ensure that sufficient data is available for sampling, packets are
   padded so that the combined lengths of the encoded packet number and
   protected payload is at least 4 bytes longer than the sample required
   for header protection.  The ciphersuites defined in [TLS13] - other
   than TLS_AES_128_CCM_8_SHA256, for which a header protection scheme
   is not defined in this document - have 16-byte expansions and 16-byte
   header protection samples.  This results in needing at least 3 bytes
   of frames in the unprotected payload if the packet number is encoded
   on a single byte, or 2 bytes of frames for a 2-byte packet number
   encoding.

   The sampled ciphertext for a packet with a short header can be
   determined by the following pseudocode:

   sample_offset = 1 + len(connection_id) + 4

   sample = packet[sample_offset..sample_offset+sample_length]

   For example, for a packet with a short header, an 8 byte connection
   ID, and protected with AEAD_AES_128_GCM, the sample takes bytes 13 to
   28 inclusive (using zero-based indexing).

   A packet with a long header is sampled in the same way, noting that
   multiple QUIC packets might be included in the same UDP datagram and
   that each one is handled separately.

   sample_offset = 7 + len(destination_connection_id) +
                       len(source_connection_id) +
                       len(payload_length) + 4
   if packet_type == Initial:
       sample_offset += len(token_length) +
                        len(token)

   sample = packet[sample_offset..sample_offset+sample_length]








Thomson & Turner        Expires 11 December 2020               [Page 27]

Internet-Draft          Using TLS to Secure QUIC               June 2020


5.4.3.  AES-Based Header Protection

   This section defines the packet protection algorithm for
   AEAD_AES_128_GCM, AEAD_AES_128_CCM, and AEAD_AES_256_GCM.
   AEAD_AES_128_GCM and AEAD_AES_128_CCM use 128-bit AES [AES] in
   electronic code-book (ECB) mode.  AEAD_AES_256_GCM uses 256-bit AES
   in ECB mode.

   This algorithm samples 16 bytes from the packet ciphertext.  This
   value is used as the input to AES-ECB.  In pseudocode:

   mask = AES-ECB(hp_key, sample)

5.4.4.  ChaCha20-Based Header Protection

   When AEAD_CHACHA20_POLY1305 is in use, header protection uses the raw
   ChaCha20 function as defined in Section 2.4 of [CHACHA].  This uses a
   256-bit key and 16 bytes sampled from the packet protection output.

   The first 4 bytes of the sampled ciphertext are the block counter.  A
   ChaCha20 implementation could take a 32-bit integer in place of a
   byte sequence, in which case the byte sequence is interpreted as a
   little-endian value.

   The remaining 12 bytes are used as the nonce.  A ChaCha20
   implementation might take an array of three 32-bit integers in place
   of a byte sequence, in which case the nonce bytes are interpreted as
   a sequence of 32-bit little-endian integers.

   The encryption mask is produced by invoking ChaCha20 to protect 5
   zero bytes.  In pseudocode:

   counter = sample[0..3]
   nonce = sample[4..15]
   mask = ChaCha20(hp_key, counter, nonce, {0,0,0,0,0})

5.5.  Receiving Protected Packets

   Once an endpoint successfully receives a packet with a given packet
   number, it MUST discard all packets in the same packet number space
   with higher packet numbers if they cannot be successfully unprotected
   with either the same key, or - if there is a key update - the next
   packet protection key (see Section 6).  Similarly, a packet that
   appears to trigger a key update, but cannot be unprotected
   successfully MUST be discarded.






Thomson & Turner        Expires 11 December 2020               [Page 28]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Failure to unprotect a packet does not necessarily indicate the
   existence of a protocol error in a peer or an attack.  The truncated
   packet number encoding used in QUIC can cause packet numbers to be
   decoded incorrectly if they are delayed significantly.

5.6.  Use of 0-RTT Keys

   If 0-RTT keys are available (see Section 4.6), the lack of replay
   protection means that restrictions on their use are necessary to
   avoid replay attacks on the protocol.

   A client MUST only use 0-RTT keys to protect data that is idempotent.
   A client MAY wish to apply additional restrictions on what data it
   sends prior to the completion of the TLS handshake.  A client
   otherwise treats 0-RTT keys as equivalent to 1-RTT keys, except that
   it MUST NOT send ACKs with 0-RTT keys.

   A client that receives an indication that its 0-RTT data has been
   accepted by a server can send 0-RTT data until it receives all of the
   server's handshake messages.  A client SHOULD stop sending 0-RTT data
   if it receives an indication that 0-RTT data has been rejected.

   A server MUST NOT use 0-RTT keys to protect packets; it uses 1-RTT
   keys to protect acknowledgements of 0-RTT packets.  A client MUST NOT
   attempt to decrypt 0-RTT packets it receives and instead MUST discard
   them.

   Once a client has installed 1-RTT keys, it MUST NOT send any more
   0-RTT packets.

   Note:  0-RTT data can be acknowledged by the server as it receives
      it, but any packets containing acknowledgments of 0-RTT data
      cannot have packet protection removed by the client until the TLS
      handshake is complete.  The 1-RTT keys necessary to remove packet
      protection cannot be derived until the client receives all server
      handshake messages.

5.7.  Receiving Out-of-Order Protected Frames

   Due to reordering and loss, protected packets might be received by an
   endpoint before the final TLS handshake messages are received.  A
   client will be unable to decrypt 1-RTT packets from the server,
   whereas a server will be able to decrypt 1-RTT packets from the
   client.  Endpoints in either role MUST NOT decrypt 1-RTT packets from
   their peer prior to completing the handshake.






Thomson & Turner        Expires 11 December 2020               [Page 29]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Even though 1-RTT keys are available to a server after receiving the
   first handshake messages from a client, it is missing assurances on
   the client state:

   *  The client is not authenticated, unless the server has chosen to
      use a pre-shared key and validated the client's pre-shared key
      binder; see Section 4.2.11 of [TLS13].

   *  The client has not demonstrated liveness, unless a RETRY packet
      was used.

   *  Any received 0-RTT data that the server responds to might be due
      to a replay attack.

   Therefore, the server's use of 1-RTT keys MUST be limited to sending
   data before the handshake is complete.  A server MUST NOT process
   incoming 1-RTT protected packets before the TLS handshake is
   complete.  Because sending acknowledgments indicates that all frames
   in a packet have been processed, a server cannot send acknowledgments
   for 1-RTT packets until the TLS handshake is complete.  Received
   packets protected with 1-RTT keys MAY be stored and later decrypted
   and used once the handshake is complete.

   Note:  TLS implementations might provide all 1-RTT secrets prior to
      handshake completion.  Even where QUIC implementations have 1-RTT
      read keys, those keys cannot be used prior to completing the
      handshake.

   The requirement for the server to wait for the client Finished
   message creates a dependency on that message being delivered.  A
   client can avoid the potential for head-of-line blocking that this
   implies by sending its 1-RTT packets coalesced with a handshake
   packet containing a copy of the CRYPTO frame that carries the
   Finished message, until one of the handshake packets is acknowledged.
   This enables immediate server processing for those packets.

   A server could receive packets protected with 0-RTT keys prior to
   receiving a TLS ClientHello.  The server MAY retain these packets for
   later decryption in anticipation of receiving a ClientHello.

5.8.  Retry Packet Integrity

   Retry packets (see the Retry Packet section of [QUIC-TRANSPORT])
   carry a Retry Integrity Tag that provides two properties: it allows
   discarding packets that have accidentally been corrupted by the
   network, and it diminishes off-path attackers' ability to send valid
   Retry packets.




Thomson & Turner        Expires 11 December 2020               [Page 30]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   The Retry Integrity Tag is a 128-bit field that is computed as the
   output of AEAD_AES_128_GCM [AEAD] used with the following inputs:

   *  The secret key, K, is 128 bits equal to
      0xccce187ed09a09d05728155a6cb96be1.

   *  The nonce, N, is 96 bits equal to 0xe54930f97f2136f0530a8c1c.

   *  The plaintext, P, is empty.

   *  The associated data, A, is the contents of the Retry Pseudo-
      Packet, as illustrated in Figure 8:

   The secret key and the nonce are values derived by calling HKDF-
   Expand-Label using
   0x8b0d37eb8535022ebc8d76a207d80df22646ec06dc809642c30a8baa2baaff4c as
   the secret, with labels being "quic key" and "quic iv" (Section 5.1).

   Retry Pseudo-Packet {
     ODCID Length (8),
     Original Destination Connection ID (0..160),
     Header Form (1) = 1,
     Fixed Bit (1) = 1,
     Long Packet Type (2) = 3,
     Type-Specific Bits (4),
     Version (32),
     DCID Len (8),
     Destination Connection ID (0..160),
     SCID Len (8),
     Retry Token (..),
   }

                       Figure 8: Retry Pseudo-Packet

   The Retry Pseudo-Packet is not sent over the wire.  It is computed by
   taking the transmitted Retry packet, removing the Retry Integrity Tag
   and prepending the two following fields:

   ODCID Length:  The ODCID Len contains the length in bytes of the
      Original Destination Connection ID field that follows it, encoded
      as an 8-bit unsigned integer.

   Original Destination Connection ID:  The Original Destination
      Connection ID contains the value of the Destination Connection ID
      from the Initial packet that this Retry is in response to.  The
      length of this field is given in ODCID Len. The presence of this
      field mitigates an off-path attacker's ability to inject a Retry
      packet.



Thomson & Turner        Expires 11 December 2020               [Page 31]

Internet-Draft          Using TLS to Secure QUIC               June 2020


6.  Key Update

   Once the handshake is confirmed (see Section 4.1.2), an endpoint MAY
   initiate a key update.

   The Key Phase bit indicates which packet protection keys are used to
   protect the packet.  The Key Phase bit is initially set to 0 for the
   first set of 1-RTT packets and toggled to signal each subsequent key
   update.

   The Key Phase bit allows a recipient to detect a change in keying
   material without needing to receive the first packet that triggered
   the change.  An endpoint that notices a changed Key Phase bit updates
   keys and decrypts the packet that contains the changed value.

   This mechanism replaces the TLS KeyUpdate message.  Endpoints MUST
   NOT send a TLS KeyUpdate message.  Endpoints MUST treat the receipt
   of a TLS KeyUpdate message as a connection error of type 0x10a,
   equivalent to a fatal TLS alert of unexpected_message (see
   Section 4.10).

   Figure 9 shows a key update process, where the initial set of keys
   used (identified with @M) are replaced by updated keys (identified
   with @N).  The value of the Key Phase bit is indicated in brackets
   [].

      Initiating Peer                    Responding Peer

   @M [0] QUIC Packets

   ... Update to @N
   @N [1] QUIC Packets
                         -------->
                                            Update to @N ...
                                         QUIC Packets [1] @N
                         <--------
                                         QUIC Packets [1] @N
                                       containing ACK
                         <--------
   ... Key Update Permitted

   @N [1] QUIC Packets
            containing ACK for @N packets
                         -------->
                                    Key Update Permitted ...

                            Figure 9: Key Update




Thomson & Turner        Expires 11 December 2020               [Page 32]

Internet-Draft          Using TLS to Secure QUIC               June 2020


6.1.  Initiating a Key Update

   Endpoints maintain separate read and write secrets for packet
   protection.  An endpoint initiates a key update by updating its
   packet protection write secret and using that to protect new packets.
   The endpoint creates a new write secret from the existing write
   secret as performed in Section 7.2 of [TLS13].  This uses the KDF
   function provided by TLS with a label of "quic ku".  The
   corresponding key and IV are created from that secret as defined in
   Section 5.1.  The header protection key is not updated.

   For example, to update write keys with TLS 1.3, HKDF-Expand-Label is
   used as:

   secret_<n+1> = HKDF-Expand-Label(secret_<n>, "quic ku",
                                    "", Hash.length)

   The endpoint toggles the value of the Key Phase bit and uses the
   updated key and IV to protect all subsequent packets.

   An endpoint MUST NOT initiate a key update prior to having confirmed
   the handshake (Section 4.1.2).  An endpoint MUST NOT initiate a
   subsequent key update prior unless it has received an acknowledgment
   for a packet that was sent protected with keys from the current key
   phase.  This ensures that keys are available to both peers before
   another key update can be initiated.  This can be implemented by
   tracking the lowest packet number sent with each key phase, and the
   highest acknowledged packet number in the 1-RTT space: once the
   latter is higher than or equal to the former, another key update can
   be initiated.

   Note:  Keys of packets other than the 1-RTT packets are never
      updated; their keys are derived solely from the TLS handshake
      state.

   The endpoint that initiates a key update also updates the keys that
   it uses for receiving packets.  These keys will be needed to process
   packets the peer sends after updating.

   An endpoint SHOULD retain old keys so that packets sent by its peer
   prior to receiving the key update can be processed.  Discarding old
   keys too early can cause delayed packets to be discarded.  Discarding
   packets will be interpreted as packet loss by the peer and could
   adversely affect performance.







Thomson & Turner        Expires 11 December 2020               [Page 33]

Internet-Draft          Using TLS to Secure QUIC               June 2020


6.2.  Responding to a Key Update

   A peer is permitted to initiate a key update after receiving an
   acknowledgement of a packet in the current key phase.  An endpoint
   detects a key update when processing a packet with a key phase that
   differs from the value last used to protect the last packet it sent.
   To process this packet, the endpoint uses the next packet protection
   key and IV.  See Section 6.3 for considerations about generating
   these keys.

   If a packet is successfully processed using the next key and IV, then
   the peer has initiated a key update.  The endpoint MUST update its
   send keys to the corresponding key phase in response, as described in
   Section 6.1.  Sending keys MUST be updated before sending an
   acknowledgement for the packet that was received with updated keys.
   By acknowledging the packet that triggered the key update in a packet
   protected with the updated keys, the endpoint signals that the key
   update is complete.

   An endpoint can defer sending the packet or acknowledgement according
   to its normal packet sending behaviour; it is not necessary to
   immediately generate a packet in response to a key update.  The next
   packet sent by the endpoint will use the updated keys.  The next
   packet that contains an acknowledgement will cause the key update to
   be completed.  If an endpoint detects a second update before it has
   sent any packets with updated keys containing an acknowledgement for
   the packet that initiated the key update, it indicates that its peer
   has updated keys twice without awaiting confirmation.  An endpoint
   MAY treat consecutive key updates as a connection error of type
   KEY_UPDATE_ERROR.

   An endpoint that receives an acknowledgement that is carried in a
   packet protected with old keys where any acknowledged packet was
   protected with newer keys MAY treat that as a connection error of
   type KEY_UPDATE_ERROR.  This indicates that a peer has received and
   acknowledged a packet that initiates a key update, but has not
   updated keys in response.

6.3.  Timing of Receive Key Generation

   Endpoints responding to an apparent key update MUST NOT generate a
   timing side-channel signal that might indicate that the Key Phase bit
   was invalid (see Section 9.4).  Endpoints can use dummy packet
   protection keys in place of discarded keys when key updates are not
   yet permitted.  Using dummy keys will generate no variation in the
   timing signal produced by attempting to remove packet protection, and
   results in all packets with an invalid Key Phase bit being rejected.




Thomson & Turner        Expires 11 December 2020               [Page 34]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   The process of creating new packet protection keys for receiving
   packets could reveal that a key update has occurred.  An endpoint MAY
   perform this process as part of packet processing, but this creates a
   timing signal that can be used by an attacker to learn when key
   updates happen and thus the value of the Key Phase bit in certain
   packets.  Endpoints MAY instead defer the creation of the next set of
   receive packet protection keys until some time after a key update
   completes, up to three times the PTO; see Section 6.5.

   Once generated, the next set of packet protection keys SHOULD be
   retained, even if the packet that was received was subsequently
   discarded.  Packets containing apparent key updates are easy to forge
   and - while the process of key update does not require significant
   effort - triggering this process could be used by an attacker for
   DoS.

   For this reason, endpoints MUST be able to retain two sets of packet
   protection keys for receiving packets: the current and the next.
   Retaining the previous keys in addition to these might improve
   performance, but this is not essential.

6.4.  Sending with Updated Keys

   An endpoint always sends packets that are protected with the newest
   keys.  Keys used for packet protection can be discarded immediately
   after switching to newer keys.

   Packets with higher packet numbers MUST be protected with either the
   same or newer packet protection keys than packets with lower packet
   numbers.  An endpoint that successfully removes protection with old
   keys when newer keys were used for packets with lower packet numbers
   MUST treat this as a connection error of type KEY_UPDATE_ERROR.

6.5.  Receiving with Different Keys

   For receiving packets during a key update, packets protected with
   older keys might arrive if they were delayed by the network.
   Retaining old packet protection keys allows these packets to be
   successfully processed.

   As packets protected with keys from the next key phase use the same
   Key Phase value as those protected with keys from the previous key
   phase, it can be necessary to distinguish between the two.  This can
   be done using packet numbers.  A recovered packet number that is
   lower than any packet number from the current key phase uses the
   previous packet protection keys; a recovered packet number that is
   higher than any packet number from the current key phase requires the
   use of the next packet protection keys.



Thomson & Turner        Expires 11 December 2020               [Page 35]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Some care is necessary to ensure that any process for selecting
   between previous, current, and next packet protection keys does not
   expose a timing side channel that might reveal which keys were used
   to remove packet protection.  See Section 9.5 for more information.

   Alternatively, endpoints can retain only two sets of packet
   protection keys, swapping previous for next after enough time has
   passed to allow for reordering in the network.  In this case, the Key
   Phase bit alone can be used to select keys.

   An endpoint MAY allow a period of approximately the Probe Timeout
   (PTO; see [QUIC-RECOVERY]) after a key update before it creates the
   next set of packet protection keys.  These updated keys MAY replace
   the previous keys at that time.  With the caveat that PTO is a
   subjective measure - that is, a peer could have a different view of
   the RTT - this time is expected to be long enough that any reordered
   packets would be declared lost by a peer even if they were
   acknowledged and short enough to allow for subsequent key updates.

   Endpoints need to allow for the possibility that a peer might not be
   able to decrypt packets that initiate a key update during the period
   when it retains old keys.  Endpoints SHOULD wait three times the PTO
   before initiating a key update after receiving an acknowledgment that
   confirms that the previous key update was received.  Failing to allow
   sufficient time could lead to packets being discarded.

   An endpoint SHOULD retain old read keys for no more than three times
   the PTO.  After this period, old read keys and their corresponding
   secrets SHOULD be discarded.

6.6.  Minimum Key Update Frequency

   Key updates MUST be initiated before usage limits on packet
   protection keys are exceeded.  For the cipher suites mentioned in
   this document, the limits in Section 5.5 of [TLS13] apply.  [TLS13]
   does not specify a limit for AEAD_AES_128_CCM, but the analysis in
   Appendix B shows that a limit of 2^23 packets can be used to obtain
   the same confidentiality protection as the limits specified in TLS.

   The usage limits defined in TLS 1.3 exist for protection against
   attacks on confidentiality and apply to successful applications of
   AEAD protection.  The integrity protections in authenticated
   encryption also depend on limiting the number of attempts to forge
   packets.  TLS achieves this by closing connections after any record
   fails an authentication check.  In comparison, QUIC ignores any
   packet that cannot be authenticated, allowing multiple forgery
   attempts.




Thomson & Turner        Expires 11 December 2020               [Page 36]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Endpoints MUST count the number of received packets that fail
   authentication for each set of keys.  If the number of packets that
   fail authentication with the same key exceeds a limit that is
   specific to the AEAD in use, the endpoint MUST stop using those keys.
   Endpoints MUST initiate a key update before reaching this limit.  If
   a key update is not possible, the endpoint MUST immediately close the
   connection.  Applying a limit reduces the probability that an
   attacker is able to successfully forge a packet; see [AEBounds] and
   [ROBUST].

   Note:  Due to the way that header protection protects the Key Phase,
      packets that are discarded are likely to have an even distribution
      of both Key Phase values.  This means that packets that fail
      authentication will often use the packet protection keys from the
      next key phase.  It is therefore necessary to also track the
      number of packets that fail authentication with the next set of
      packet protection keys.  To avoid exhaustion of both sets of keys,
      it might be necessary to initiate two key updates in succession.

   For AEAD_AES_128_GCM, AEAD_AES_256_GCM, and AEAD_CHACHA20_POLY1305,
   the limit on the number of packets that fail authentication is 2^36.
   Note that the analysis in [AEBounds] supports a higher limit for the
   AEAD_AES_128_GCM and AEAD_AES_256_GCM, but this specification
   recommends a lower limit.  For AEAD_AES_128_CCM, the limit on the
   number of packets that fail authentication is 2^23.5; see Appendix B.

   Note:  These limits were originally calculated using assumptions
      about the limits on TLS record size.  The maximum size of a TLS
      record is 2^14 bytes.  In comparison, QUIC packets can be up to
      2^16 bytes.  However, it is expected that QUIC packets will
      generally be smaller than TLS records.  Where packets might be
      larger than 2^14 bytes in length, smaller limits might be needed.

   Any TLS cipher suite that is specified for use with QUIC MUST define
   limits on the use of the associated AEAD function that preserves
   margins for confidentiality and integrity.  That is, limits MUST be
   specified for the number of packets that can be authenticated and for
   the number packets that can fail authentication.  Providing a
   reference to any analysis upon which values are based - and any
   assumptions used in that analysis - allows limits to be adapted to
   varying usage conditions.

6.7.  Key Update Error Code

   The KEY_UPDATE_ERROR error code (0xE) is used to signal errors
   related to key updates.





Thomson & Turner        Expires 11 December 2020               [Page 37]

Internet-Draft          Using TLS to Secure QUIC               June 2020


7.  Security of Initial Messages

   Initial packets are not protected with a secret key, so they are
   subject to potential tampering by an attacker.  QUIC provides
   protection against attackers that cannot read packets, but does not
   attempt to provide additional protection against attacks where the
   attacker can observe and inject packets.  Some forms of tampering -
   such as modifying the TLS messages themselves - are detectable, but
   some - such as modifying ACKs - are not.

   For example, an attacker could inject a packet containing an ACK
   frame that makes it appear that a packet had not been received or to
   create a false impression of the state of the connection (e.g., by
   modifying the ACK Delay).  Note that such a packet could cause a
   legitimate packet to be dropped as a duplicate.  Implementations
   SHOULD use caution in relying on any data which is contained in
   Initial packets that is not otherwise authenticated.

   It is also possible for the attacker to tamper with data that is
   carried in Handshake packets, but because that tampering requires
   modifying TLS handshake messages, that tampering will cause the TLS
   handshake to fail.

8.  QUIC-Specific Adjustments to the TLS Handshake

   QUIC uses the TLS handshake for more than just negotiation of
   cryptographic parameters.  The TLS handshake provides preliminary
   values for QUIC transport parameters and allows a server to perform
   return routability checks on clients.

8.1.  Protocol Negotiation

   QUIC requires that the cryptographic handshake provide authenticated
   protocol negotiation.  TLS uses Application Layer Protocol
   Negotiation (ALPN) [ALPN] to select an application protocol.  Unless
   another mechanism is used for agreeing on an application protocol,
   endpoints MUST use ALPN for this purpose.

   When using ALPN, endpoints MUST immediately close a connection (see
   Section 10.3 of [QUIC-TRANSPORT]) with a no_application_protocol TLS
   alert (QUIC error code 0x178; see Section 4.10) if an application
   protocol is not negotiated.  While [ALPN] only specifies that servers
   use this alert, QUIC clients MUST use error 0x178 to terminate a
   connection when ALPN negotiation fails.

   An application protocol MAY restrict the QUIC versions that it can
   operate over.  Servers MUST select an application protocol compatible
   with the QUIC version that the client has selected.  The server MUST



Thomson & Turner        Expires 11 December 2020               [Page 38]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   treat the inability to select a compatible application protocol as a
   connection error of type 0x178 (no_application_protocol).  Similarly,
   a client MUST treat the selection of an incompatible application
   protocol by a server as a connection error of type 0x178.

8.2.  QUIC Transport Parameters Extension

   QUIC transport parameters are carried in a TLS extension.  Different
   versions of QUIC might define a different method for negotiating
   transport configuration.

   Including transport parameters in the TLS handshake provides
   integrity protection for these values.

      enum {
         quic_transport_parameters(0xffa5), (65535)
      } ExtensionType;

   The extension_data field of the quic_transport_parameters extension
   contains a value that is defined by the version of QUIC that is in
   use.

   The quic_transport_parameters extension is carried in the ClientHello
   and the EncryptedExtensions messages during the handshake.  Endpoints
   MUST send the quic_transport_parameters extension; endpoints that
   receive ClientHello or EncryptedExtensions messages without the
   quic_transport_parameters extension MUST close the connection with an
   error of type 0x16d (equivalent to a fatal TLS missing_extension
   alert, see Section 4.10).

   While the transport parameters are technically available prior to the
   completion of the handshake, they cannot be fully trusted until the
   handshake completes, and reliance on them should be minimized.
   However, any tampering with the parameters will cause the handshake
   to fail.

   Endpoints MUST NOT send this extension in a TLS connection that does
   not use QUIC (such as the use of TLS with TCP defined in [TLS13]).  A
   fatal unsupported_extension alert MUST be sent by an implementation
   that supports this extension if the extension is received when the
   transport is not QUIC.

8.3.  Removing the EndOfEarlyData Message

   The TLS EndOfEarlyData message is not used with QUIC.  QUIC does not
   rely on this message to mark the end of 0-RTT data or to signal the
   change to Handshake keys.




Thomson & Turner        Expires 11 December 2020               [Page 39]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Clients MUST NOT send the EndOfEarlyData message.  A server MUST
   treat receipt of a CRYPTO frame in a 0-RTT packet as a connection
   error of type PROTOCOL_VIOLATION.

   As a result, EndOfEarlyData does not appear in the TLS handshake
   transcript.

8.4.  Prohibit TLS Middlebox Compatibility Mode

   Appendix D.4 of [TLS13] describes an alteration to the TLS 1.3
   handshake as a workaround for bugs in some middleboxes.  The TLS 1.3
   middlebox compatibility mode involves setting the legacy_session_id
   field to a 32-byte value in the ClientHello and ServerHello, then
   sending a change_cipher_spec record.  Both field and record carry no
   semantic content and are ignored.

   This mode has no use in QUIC as it only applies to middleboxes that
   interfere with TLS over TCP.  QUIC also provides no means to carry a
   change_cipher_spec record.  A client MUST NOT request the use of the
   TLS 1.3 compatibility mode.  A server SHOULD treat the receipt of a
   TLS ClientHello with a non-empty legacy_session_id field as a
   connection error of type PROTOCOL_VIOLATION.

9.  Security Considerations

   All of the security considerations that apply to TLS also apply to
   the use of TLS in QUIC.  Reading all of [TLS13] and its appendices is
   the best way to gain an understanding of the security properties of
   QUIC.

   This section summarizes some of the more important security aspects
   specific to the TLS integration, though there are many security-
   relevant details in the remainder of the document.

9.1.  Session Linkability

   Use of TLS session tickets allows servers and possibly other entities
   to correlate connections made by the same client; see Section 4.5 for
   details.

9.2.  Replay Attacks with 0-RTT

   As described in Section 8 of [TLS13], use of TLS early data comes
   with an exposure to replay attack.  The use of 0-RTT in QUIC is
   similarly vulnerable to replay attack.






Thomson & Turner        Expires 11 December 2020               [Page 40]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   Endpoints MUST implement and use the replay protections described in
   [TLS13], however it is recognized that these protections are
   imperfect.  Therefore, additional consideration of the risk of replay
   is needed.

   QUIC is not vulnerable to replay attack, except via the application
   protocol information it might carry.  The management of QUIC protocol
   state based on the frame types defined in [QUIC-TRANSPORT] is not
   vulnerable to replay.  Processing of QUIC frames is idempotent and
   cannot result in invalid connection states if frames are replayed,
   reordered or lost.  QUIC connections do not produce effects that last
   beyond the lifetime of the connection, except for those produced by
   the application protocol that QUIC serves.

   Note:  TLS session tickets and address validation tokens are used to
      carry QUIC configuration information between connections.  These
      MUST NOT be used to carry application semantics.  The potential
      for reuse of these tokens means that they require stronger
      protections against replay.

   A server that accepts 0-RTT on a connection incurs a higher cost than
   accepting a connection without 0-RTT.  This includes higher
   processing and computation costs.  Servers need to consider the
   probability of replay and all associated costs when accepting 0-RTT.

   Ultimately, the responsibility for managing the risks of replay
   attacks with 0-RTT lies with an application protocol.  An application
   protocol that uses QUIC MUST describe how the protocol uses 0-RTT and
   the measures that are employed to protect against replay attack.  An
   analysis of replay risk needs to consider all QUIC protocol features
   that carry application semantics.

   Disabling 0-RTT entirely is the most effective defense against replay
   attack.

   QUIC extensions MUST describe how replay attacks affect their
   operation, or prohibit their use in 0-RTT.  Application protocols
   MUST either prohibit the use of extensions that carry application
   semantics in 0-RTT or provide replay mitigation strategies.

9.3.  Packet Reflection Attack Mitigation

   A small ClientHello that results in a large block of handshake
   messages from a server can be used in packet reflection attacks to
   amplify the traffic generated by an attacker.






Thomson & Turner        Expires 11 December 2020               [Page 41]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   QUIC includes three defenses against this attack.  First, the packet
   containing a ClientHello MUST be padded to a minimum size.  Second,
   if responding to an unverified source address, the server is
   forbidden to send more than three UDP datagrams in its first flight
   (see Section 8.1 of [QUIC-TRANSPORT]).  Finally, because
   acknowledgements of Handshake packets are authenticated, a blind
   attacker cannot forge them.  Put together, these defenses limit the
   level of amplification.

9.4.  Header Protection Analysis

   [NAN] analyzes authenticated encryption algorithms which provide
   nonce privacy, referred to as "Hide Nonce" (HN) transforms.  The
   general header protection construction in this document is one of
   those algorithms (HN1).  Header protection uses the output of the
   packet protection AEAD to derive "sample", and then encrypts the
   header field using a pseudorandom function (PRF) as follows:

   protected_field = field XOR PRF(hp_key, sample)

   The header protection variants in this document use a pseudorandom
   permutation (PRP) in place of a generic PRF.  However, since all PRPs
   are also PRFs [IMC], these variants do not deviate from the HN1
   construction.

   As "hp_key" is distinct from the packet protection key, it follows
   that header protection achieves AE2 security as defined in [NAN] and
   therefore guarantees privacy of "field", the protected packet header.
   Future header protection variants based on this construction MUST use
   a PRF to ensure equivalent security guarantees.

   Use of the same key and ciphertext sample more than once risks
   compromising header protection.  Protecting two different headers
   with the same key and ciphertext sample reveals the exclusive OR of
   the protected fields.  Assuming that the AEAD acts as a PRF, if L
   bits are sampled, the odds of two ciphertext samples being identical
   approach 2^(-L/2), that is, the birthday bound.  For the algorithms
   described in this document, that probability is one in 2^64.

   To prevent an attacker from modifying packet headers, the header is
   transitively authenticated using packet protection; the entire packet
   header is part of the authenticated additional data.  Protected
   fields that are falsified or modified can only be detected once the
   packet protection is removed.







Thomson & Turner        Expires 11 December 2020               [Page 42]

Internet-Draft          Using TLS to Secure QUIC               June 2020


9.5.  Header Protection Timing Side-Channels

   An attacker could guess values for packet numbers or Key Phase and
   have an endpoint confirm guesses through timing side channels.
   Similarly, guesses for the packet number length can be trialed and
   exposed.  If the recipient of a packet discards packets with
   duplicate packet numbers without attempting to remove packet
   protection they could reveal through timing side-channels that the
   packet number matches a received packet.  For authentication to be
   free from side-channels, the entire process of header protection
   removal, packet number recovery, and packet protection removal MUST
   be applied together without timing and other side-channels.

   For the sending of packets, construction and protection of packet
   payloads and packet numbers MUST be free from side-channels that
   would reveal the packet number or its encoded size.

   During a key update, the time taken to generate new keys could reveal
   through timing side-channels that a key update has occurred.
   Alternatively, where an attacker injects packets this side-channel
   could reveal the value of the Key Phase on injected packets.  After
   receiving a key update, an endpoint SHOULD generate and save the next
   set of receive packet protection keys, as described in Section 6.3.
   By generating new keys before a key update is received, receipt of
   packets will not create timing signals that leak the value of the Key
   Phase.

   This depends on not doing this key generation during packet
   processing and it can require that endpoints maintain three sets of
   packet protection keys for receiving: for the previous key phase, for
   the current key phase, and for the next key phase.  Endpoints can
   instead choose to defer generation of the next receive packet
   protection keys until they discard old keys so that only two sets of
   receive keys need to be retained at any point in time.

9.6.  Key Diversity

   In using TLS, the central key schedule of TLS is used.  As a result
   of the TLS handshake messages being integrated into the calculation
   of secrets, the inclusion of the QUIC transport parameters extension
   ensures that handshake and 1-RTT keys are not the same as those that
   might be produced by a server running TLS over TCP.  To avoid the
   possibility of cross-protocol key synchronization, additional
   measures are provided to improve key separation.

   The QUIC packet protection keys and IVs are derived using a different
   label than the equivalent keys in TLS.




Thomson & Turner        Expires 11 December 2020               [Page 43]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   To preserve this separation, a new version of QUIC SHOULD define new
   labels for key derivation for packet protection key and IV, plus the
   header protection keys.  This version of QUIC uses the string "quic".
   Other versions can use a version-specific label in place of that
   string.

   The initial secrets use a key that is specific to the negotiated QUIC
   version.  New QUIC versions SHOULD define a new salt value used in
   calculating initial secrets.

10.  IANA Considerations

   This document does not create any new IANA registries, but it
   registers the values in the following registries:

   *  TLS ExtensionType Values Registry [TLS-REGISTRIES] - IANA is to
      register the quic_transport_parameters extension found in
      Section 8.2.  The Recommended column is to be marked Yes.  The TLS
      1.3 Column is to include CH and EE.

   *  QUIC Transport Error Codes Registry [QUIC-TRANSPORT] - IANA is to
      register the KEY_UPDATE_ERROR (0xE), as described in Section 6.7.

11.  References

11.1.  Normative References

   [AEAD]     McGrew, D., "An Interface and Algorithms for Authenticated
              Encryption", RFC 5116, DOI 10.17487/RFC5116, January 2008,
              <https://www.rfc-editor.org/info/rfc5116>.

   [AES]      "Advanced encryption standard (AES)",
              DOI 10.6028/nist.fips.197, National Institute of Standards
              and Technology report, November 2001,
              <https://doi.org/10.6028/nist.fips.197>.

   [ALPN]     Friedl, S., Popov, A., Langley, A., and E. Stephan,
              "Transport Layer Security (TLS) Application-Layer Protocol
              Negotiation Extension", RFC 7301, DOI 10.17487/RFC7301,
              July 2014, <https://www.rfc-editor.org/info/rfc7301>.

   [CHACHA]   Nir, Y. and A. Langley, "ChaCha20 and Poly1305 for IETF
              Protocols", RFC 8439, DOI 10.17487/RFC8439, June 2018,
              <https://www.rfc-editor.org/info/rfc8439>.







Thomson & Turner        Expires 11 December 2020               [Page 44]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   [QUIC-RECOVERY]
              Iyengar, J., Ed. and I. Swett, Ed., "QUIC Loss Detection
              and Congestion Control", Work in Progress, Internet-Draft,
              draft-ietf-quic-recovery-29, 9 June 2020,
              <https://tools.ietf.org/html/draft-ietf-quic-recovery-29>.

   [QUIC-TRANSPORT]
              Iyengar, J., Ed. and M. Thomson, Ed., "QUIC: A UDP-Based
              Multiplexed and Secure Transport", Work in Progress,
              Internet-Draft, draft-ietf-quic-transport-29, 9 June 2020,
              <https://tools.ietf.org/html/draft-ietf-quic-transport-
              29>.

   [RFC2119]  Bradner, S., "Key words for use in RFCs to Indicate
              Requirement Levels", BCP 14, RFC 2119,
              DOI 10.17487/RFC2119, March 1997,
              <https://www.rfc-editor.org/info/rfc2119>.

   [RFC8174]  Leiba, B., "Ambiguity of Uppercase vs Lowercase in RFC
              2119 Key Words", BCP 14, RFC 8174, DOI 10.17487/RFC8174,
              May 2017, <https://www.rfc-editor.org/info/rfc8174>.

   [SHA]      Dang, Q., "Secure Hash Standard",
              DOI 10.6028/nist.fips.180-4, National Institute of
              Standards and Technology report, July 2015,
              <https://doi.org/10.6028/nist.fips.180-4>.

   [TLS-REGISTRIES]
              Salowey, J. and S. Turner, "IANA Registry Updates for TLS
              and DTLS", RFC 8447, DOI 10.17487/RFC8447, August 2018,
              <https://www.rfc-editor.org/info/rfc8447>.

   [TLS13]    Rescorla, E., "The Transport Layer Security (TLS) Protocol
              Version 1.3", RFC 8446, DOI 10.17487/RFC8446, August 2018,
              <https://www.rfc-editor.org/info/rfc8446>.

11.2.  Informative References

   [AEBounds] Luykx, A. and K. Paterson, "Limits on Authenticated
              Encryption Use in TLS", 8 March 2016,
              <http://www.isg.rhul.ac.uk/~kp/TLS-AEbounds.pdf>.

   [CCM-ANALYSIS]
              Jonsson, J., "On the Security of CTR + CBC-MAC",
              DOI 10.1007/3-540-36492-7_7, Selected Areas in
              Cryptography pp. 76-93, 2003,
              <https://doi.org/10.1007/3-540-36492-7_7>.




Thomson & Turner        Expires 11 December 2020               [Page 45]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   [HTTP2-TLS13]
              Benjamin, D., "Using TLS 1.3 with HTTP/2", RFC 8740,
              DOI 10.17487/RFC8740, February 2020,
              <https://www.rfc-editor.org/info/rfc8740>.

   [IMC]      Katz, J. and Y. Lindell, "Introduction to Modern
              Cryptography, Second Edition", ISBN 978-1466570269, 6
              November 2014.

   [NAN]      Bellare, M., Ng, R., and B. Tackmann, "Nonces Are Noticed:
              AEAD Revisited", DOI 10.1007/978-3-030-26948-7_9, Advances
              in Cryptology - CRYPTO 2019 pp. 235-265, 2019,
              <https://doi.org/10.1007/978-3-030-26948-7_9>.

   [QUIC-HTTP]
              Bishop, M., Ed., "Hypertext Transfer Protocol Version 3
              (HTTP/3)", Work in Progress, Internet-Draft, draft-ietf-
              quic-http-29, 9 June 2020,
              <https://tools.ietf.org/html/draft-ietf-quic-http-29>.

   [RFC2818]  Rescorla, E., "HTTP Over TLS", RFC 2818,
              DOI 10.17487/RFC2818, May 2000,
              <https://www.rfc-editor.org/info/rfc2818>.

   [RFC5280]  Cooper, D., Santesson, S., Farrell, S., Boeyen, S.,
              Housley, R., and W. Polk, "Internet X.509 Public Key
              Infrastructure Certificate and Certificate Revocation List
              (CRL) Profile", RFC 5280, DOI 10.17487/RFC5280, May 2008,
              <https://www.rfc-editor.org/info/rfc5280>.

   [ROBUST]   Fischlin, M., Günther, F., and C. Janson, "Robust
              Channels: Handling Unreliable Networks in the Record
              Layers of QUIC and DTLS", 21 February 2020,
              <https://www.felixguenther.info/docs/
              QUIPS2020_RobustChannels.pdf>.

Appendix A.  Sample Packet Protection

   This section shows examples of packet protection so that
   implementations can be verified incrementally.  Samples of Initial
   packets from both client and server, plus a Retry packet are defined.
   These packets use an 8-byte client-chosen Destination Connection ID
   of 0x8394c8f03e515708.  Some intermediate values are included.  All
   values are shown in hexadecimal.







Thomson & Turner        Expires 11 December 2020               [Page 46]

Internet-Draft          Using TLS to Secure QUIC               June 2020


A.1.  Keys

   The labels generated by the HKDF-Expand-Label function are:

   client in:  00200f746c73313320636c69656e7420696e00

   server in:  00200f746c7331332073657276657220696e00

   quic key:  00100e746c7331332071756963206b657900

   quic iv:  000c0d746c733133207175696320697600

   quic hp:  00100d746c733133207175696320687000

   The initial secret is common:

   initial_secret = HKDF-Extract(initial_salt, cid)
       = 1e7e7764529715b1e0ddc8e9753c6157
         6769605187793ed366f8bbf8c9e986eb

   The secrets for protecting client packets are:

   client_initial_secret
       = HKDF-Expand-Label(initial_secret, "client in", _, 32)
       = 0088119288f1d866733ceeed15ff9d50
         902cf82952eee27e9d4d4918ea371d87

   key = HKDF-Expand-Label(client_initial_secret, "quic key", _, 16)
       = 175257a31eb09dea9366d8bb79ad80ba

   iv  = HKDF-Expand-Label(client_initial_secret, "quic iv", _, 12)
       = 6b26114b9cba2b63a9e8dd4f

   hp  = HKDF-Expand-Label(client_initial_secret, "quic hp", _, 16)
       = 9ddd12c994c0698b89374a9c077a3077

   The secrets for protecting server packets are:














Thomson & Turner        Expires 11 December 2020               [Page 47]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   server_initial_secret
       = HKDF-Expand-Label(initial_secret, "server in", _, 32)
       = 006f881359244dd9ad1acf85f595bad6
         7c13f9f5586f5e64e1acae1d9ea8f616

   key = HKDF-Expand-Label(server_initial_secret, "quic key", _, 16)
       = 149d0b1662ab871fbe63c49b5e655a5d

   iv  = HKDF-Expand-Label(server_initial_secret, "quic iv", _, 12)
       = bab2b12a4c76016ace47856d

   hp  = HKDF-Expand-Label(server_initial_secret, "quic hp", _, 16)
       = c0c499a65a60024a18a250974ea01dfa

A.2.  Client Initial

   The client sends an Initial packet.  The unprotected payload of this
   packet contains the following CRYPTO frame, plus enough PADDING
   frames to make a 1162 byte payload:

   060040c4010000c003036660261ff947 cea49cce6cfad687f457cf1b14531ba1
   4131a0e8f309a1d0b9c4000006130113 031302010000910000000b0009000006
   736572766572ff01000100000a001400 12001d00170018001901000101010201
   03010400230000003300260024001d00 204cfdfcd178b784bf328cae793b136f
   2aedce005ff183d7bb14952072366470 37002b0003020304000d0020001e0403
   05030603020308040805080604010501 060102010402050206020202002d0002
   0101001c00024001

   The unprotected header includes the connection ID and a 4 byte packet
   number encoding for a packet number of 2:

   c3ff00001d088394c8f03e5157080000449e00000002

   Protecting the payload produces output that is sampled for header
   protection.  Because the header uses a 4 byte packet number encoding,
   the first 16 bytes of the protected payload is sampled, then applied
   to the header:

   sample = fb66bc5f93032b7ddd89fe0ff15d9c4f

   mask = AES-ECB(hp, sample)[0..4]
        = d64a952459

   header[0] ^= mask[0] & 0x0f
        = c5
   header[18..21] ^= mask[1..4]
        = 4a95245b
   header = c5ff00001d088394c8f03e5157080000449e4a95245b



Thomson & Turner        Expires 11 December 2020               [Page 48]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   The resulting protected packet is:

   c5ff00001d088394c8f03e5157080000 449e4a95245bfb66bc5f93032b7ddd89
   fe0ff15d9c4f7050fccdb71c1cd80512 d4431643a53aafa1b0b518b44968b18b
   8d3e7a4d04c30b3ed9410325b2abb2da fb1c12f8b70479eb8df98abcaf95dd8f
   3d1c78660fbc719f88b23c8aef6771f3 d50e10fdfb4c9d92386d44481b6c52d5
   9e5538d3d3942de9f13a7f8b702dc317 24180da9df22714d01003fc5e3d165c9
   50e630b8540fbd81c9df0ee63f949970 26c4f2e1887a2def79050ac2d86ba318
   e0b3adc4c5aa18bcf63c7cf8e85f5692 49813a2236a7e72269447cd1c755e451
   f5e77470eb3de64c8849d29282069802 9cfa18e5d66176fe6e5ba4ed18026f90
   900a5b4980e2f58e39151d5cd685b109 29636d4f02e7fad2a5a458249f5c0298
   a6d53acbe41a7fc83fa7cc01973f7a74 d1237a51974e097636b6203997f921d0
   7bc1940a6f2d0de9f5a11432946159ed 6cc21df65c4ddd1115f86427259a196c
   7148b25b6478b0dc7766e1c4d1b1f515 9f90eabc61636226244642ee148b464c
   9e619ee50a5e3ddc836227cad938987c 4ea3c1fa7c75bbf88d89e9ada642b2b8
   8fe8107b7ea375b1b64889a4e9e5c38a 1c896ce275a5658d250e2d76e1ed3a34
   ce7e3a3f383d0c996d0bed106c2899ca 6fc263ef0455e74bb6ac1640ea7bfedc
   59f03fee0e1725ea150ff4d69a7660c5 542119c71de270ae7c3ecfd1af2c4ce5
   51986949cc34a66b3e216bfe18b347e6 c05fd050f85912db303a8f054ec23e38
   f44d1c725ab641ae929fecc8e3cefa56 19df4231f5b4c009fa0c0bbc60bc75f7
   6d06ef154fc8577077d9d6a1d2bd9bf0 81dc783ece60111bea7da9e5a9748069
   d078b2bef48de04cabe3755b197d52b3 2046949ecaa310274b4aac0d008b1948
   c1082cdfe2083e386d4fd84c0ed0666d 3ee26c4515c4fee73433ac703b690a9f
   7bf278a77486ace44c489a0c7ac8dfe4 d1a58fb3a730b993ff0f0d61b4d89557
   831eb4c752ffd39c10f6b9f46d8db278 da624fd800e4af85548a294c1518893a
   8778c4f6d6d73c93df200960104e062b 388ea97dcf4016bced7f62b4f062cb6c
   04c20693d9a0e3b74ba8fe74cc012378 84f40d765ae56a51688d985cf0ceaef4
   3045ed8c3f0c33bced08537f6882613a cd3b08d665fce9dd8aa73171e2d3771a
   61dba2790e491d413d93d987e2745af2 9418e428be34941485c93447520ffe23
   1da2304d6a0fd5d07d08372202369661 59bef3cf904d722324dd852513df39ae
   030d8173908da6364786d3c1bfcb19ea 77a63b25f1e7fc661def480c5d00d444
   56269ebd84efd8e3a8b2c257eec76060 682848cbf5194bc99e49ee75e4d0d254
   bad4bfd74970c30e44b65511d4ad0e6e c7398e08e01307eeeea14e46ccd87cf3
   6b285221254d8fc6a6765c524ded0085 dca5bd688ddf722e2c0faf9d0fb2ce7a
   0c3f2cee19ca0ffba461ca8dc5d2c817 8b0762cf67135558494d2a96f1a139f0
   edb42d2af89a9c9122b07acbc29e5e72 2df8615c343702491098478a389c9872
   a10b0c9875125e257c7bfdf27eef4060 bd3d00f4c14fd3e3496c38d3c5d1a566
   8c39350effbc2d16ca17be4ce29f02ed 969504dda2a8c6b9ff919e693ee79e09
   089316e7d1d89ec099db3b2b268725d8 88536a4b8bf9aee8fb43e82a4d919d48
   43b1ca70a2d8d3f725ead1391377dcc0

A.3.  Server Initial

   The server sends the following payload in response, including an ACK
   frame, a CRYPTO frame, and no PADDING frames:






Thomson & Turner        Expires 11 December 2020               [Page 49]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   0d0000000018410a020000560303eefc e7f7b37ba1d1632e96677825ddf73988
   cfc79825df566dc5430b9a045a120013 0100002e00330024001d00209d3c940d
   89690b84d08a60993c144eca684d1081 287c834d5311bcf32bb9da1a002b0002
   0304

   The header from the server includes a new connection ID and a 2-byte
   packet number encoding for a packet number of 1:

   c1ff00001d0008f067a5502a4262b50040740001

   As a result, after protection, the header protection sample is taken
   starting from the third protected octet:

   sample = 823a5d3a1207c86ee49132824f046524
   mask   = abaaf34fdc
   header = caff00001d0008f067a5502a4262b5004074aaf2

   The final protected packet is then:

   caff00001d0008f067a5502a4262b500 4074aaf2f007823a5d3a1207c86ee491
   32824f0465243d082d868b107a38092b c80528664cbf9456ebf27673fb5fa506
   1ab573c9f001b81da028a00d52ab00b1 5bebaa70640e106cf2acd043e9c6b441
   1c0a79637134d8993701fe779e58c2fe 753d14b0564021565ea92e57bc6faf56
   dfc7a40870e6

A.4.  Retry

   This shows a Retry packet that might be sent in response to the
   Initial packet in Appendix A.2.  The integrity check includes the
   client-chosen connection ID value of 0x8394c8f03e515708, but that
   value is not included in the final Retry packet:

   ffff00001d0008f067a5502a4262b574 6f6b656ed16926d81f6f9ca2953a8aa4
   575e1e49

A.5.  ChaCha20-Poly1305 Short Header Packet

   This example shows some of the steps required to protect a packet
   with a short header.  This example uses AEAD_CHACHA20_POLY1305.

   In this example, TLS produces an application write secret from which
   a server uses HKDF-Expand-Label to produce four values: a key, an IV,
   a header protection key, and the secret that will be used after keys
   are updated (this last value is not used further in this example).







Thomson & Turner        Expires 11 December 2020               [Page 50]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   secret
       = 9ac312a7f877468ebe69422748ad00a1
         5443f18203a07d6060f688f30f21632b

   key = HKDF-Expand-Label(secret, "quic key", _, 32)
       = c6d98ff3441c3fe1b2182094f69caa2e
         d4b716b65488960a7a984979fb23e1c8

   iv  = HKDF-Expand-Label(secret, "quic iv", _, 12)
       = e0459b3474bdd0e44a41c144

   hp  = HKDF-Expand-Label(secret, "quic hp", _, 32)
       = 25a282b9e82f06f21f488917a4fc8f1b
         73573685608597d0efcb076b0ab7a7a4

   ku  = HKDF-Expand-Label(secret, "quic ku", _, 32)
       = 1223504755036d556342ee9361d25342
         1a826c9ecdf3c7148684b36b714881f9

   The following shows the steps involved in protecting a minimal packet
   with an empty Destination Connection ID.  This packet contains a
   single PING frame (that is, a payload of just 0x01) and has a packet
   number of 654360564.  In this example, using a packet number of
   length 3 (that is, 49140 is encoded) avoids having to pad the payload
   of the packet; PADDING frames would be needed if the packet number is
   encoded on fewer octets.

   pn                 = 654360564 (decimal)
   nonce              = e0459b3474bdd0e46d417eb0
   unprotected header = 4200bff4
   payload plaintext  = 01
   payload ciphertext = 655e5cd55c41f69080575d7999c25a5bfb

   The resulting ciphertext is the minimum size possible.  One byte is
   skipped to produce the sample for header protection.

   sample = 5e5cd55c41f69080575d7999c25a5bfb
   mask   = aefefe7d03
   header = 4cfe4189

   The protected packet is the smallest possible packet size of 21
   bytes.

   packet = 4cfe4189655e5cd55c41f69080575d7999c25a5bfb







Thomson & Turner        Expires 11 December 2020               [Page 51]

Internet-Draft          Using TLS to Secure QUIC               June 2020


Appendix B.  Analysis of Limits on AEAD_AES_128_CCM Usage

   TLS [TLS13] and [AEBounds] do not specify limits on usage for
   AEAD_AES_128_CCM.  However, any AEAD that is used with QUIC requires
   limits on use that ensure that both confidentiality and integrity are
   preserved.  This section documents that analysis.

   [CCM-ANALYSIS] is used as the basis of this analysis.  The results of
   that analysis are used to derive usage limits that are based on those
   chosen in [TLS13].

   This analysis uses symbols for multiplication (*), division (/), and
   exponentiation (^), plus parentheses for establishing precedence.
   The following symbols are also used:

   t:  The size of the authentication tag in bits.  For this cipher, t
      is 128.

   n:  The size of the block function in bits.  For this cipher, n is
      128.

   l:  The number of blocks in each packet (see below).

   q:  The number of genuine packets created and protected by endpoints.
      This value is the bound on the number of packets that can be
      protected before updating keys.

   v:  The number of forged packets that endpoints will accept.  This
      value is the bound on the number of forged packets that an
      endpoint can reject before updating keys.

   The analysis of AEAD_AES_128_CCM relies on a count of the number of
   block operations involved in producing each message.  For simplicity,
   and to match the analysis of other AEAD functions in [AEBounds], this
   analysis assumes a packet length of 2^10 blocks and a packet size
   limit of 2^14.

   For AEAD_AES_128_CCM, the total number of block cipher operations is
   the sum of: the length of the associated data in blocks, the length
   of the ciphertext in blocks, the length of the plaintext in blocks,
   plus 1.  In this analysis, this is simplified to a value of twice the
   length of the packet in blocks (that is, "2l = 2^11").  This
   simplification is based on the packet containing all of the
   associated data and ciphertext.  This results in a negligible 1 to 3
   block overestimation of the number of operations.






Thomson & Turner        Expires 11 December 2020               [Page 52]

Internet-Draft          Using TLS to Secure QUIC               June 2020


B.1.  Confidentiality Limits

   For confidentiality, Theorem 2 in [CCM-ANALYSIS] establishes that an
   attacker gains a distinguishing advantage over an ideal pseudorandom
   permutation (PRP) of no more than:

   (2l * q)^2 / 2^n

   For a target advantage of 2^-60, which matches that used by [TLS13],
   this results in the relation:

   q <= 2^23

   That is, endpoints cannot protect more than 2^23 packets with the
   same set of keys without causing an attacker to gain an larger
   advantage than the target of 2^-60.

B.2.  Integrity Limits

   For integrity, Theorem 1 in [CCM-ANALYSIS] establishes that an
   attacker gains an advantage over an ideal PRP of no more than:

   v / 2^t + (2l * (v + q))^2 / 2^n

   The goal is to limit this advantage to 2^-57, to match the target in
   [TLS13].  As "t" and "n" are both 128, the first term is negligible
   relative to the second, so that term can be removed without a
   significant effect on the result.  This produces the relation:

   v + q <= 2^24.5

   Using the previously-established value of 2^23 for "q" and rounding,
   this leads to an upper limit on "v" of 2^23.5.  That is, endpoints
   cannot attempt to authenticate more than 2^23.5 packets with the same
   set of keys without causing an attacker to gain an larger advantage
   than the target of 2^-57.

Appendix C.  Change Log

      *RFC Editor's Note:* Please remove this section prior to
      publication of a final version of this document.

   Issue and pull request numbers are listed with a leading octothorp.

C.1.  Since draft-ietf-quic-tls-28






Thomson & Turner        Expires 11 December 2020               [Page 53]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   *  Defined limits on the number of packets that can be protected with
      a single key and limits on the number of packets that can fail
      authentication (#3619, #3620)

C.2.  Since draft-ietf-quic-tls-27

   *  Allowed CONNECTION_CLOSE in any packet number space, with
      restrictions on use of the application-specific variant (#3430,
      #3435, #3440)

   *  Prohibit the use of the compatibility mode from TLS 1.3 (#3594,
      #3595)

C.3.  Since draft-ietf-quic-tls-26

   *  No changes

C.4.  Since draft-ietf-quic-tls-25

   *  No changes

C.5.  Since draft-ietf-quic-tls-24

   *  Rewrite key updates (#3050)

      -  Allow but don't recommend deferring key updates (#2792, #3263)

      -  More completely define received behavior (#2791)

      -  Define the label used with HKDF-Expand-Label (#3054)

C.6.  Since draft-ietf-quic-tls-23

   *  Key update text update (#3050):

      -  Recommend constant-time key replacement (#2792)

      -  Provide explicit labels for key update key derivation (#3054)

   *  Allow first Initial from a client to span multiple packets (#2928,
      #3045)

   *  PING can be sent at any encryption level (#3034, #3035)

C.7.  Since draft-ietf-quic-tls-22

   *  Update the salt used for Initial secrets (#2887, #2980)




Thomson & Turner        Expires 11 December 2020               [Page 54]

Internet-Draft          Using TLS to Secure QUIC               June 2020


C.8.  Since draft-ietf-quic-tls-21

   *  No changes

C.9.  Since draft-ietf-quic-tls-20

   *  Mandate the use of the QUIC transport parameters extension (#2528,
      #2560)

   *  Define handshake completion and confirmation; define clearer rules
      when it encryption keys should be discarded (#2214, #2267, #2673)

C.10.  Since draft-ietf-quic-tls-18

   *  Increased the set of permissible frames in 0-RTT (#2344, #2355)

   *  Transport parameter extension is mandatory (#2528, #2560)

C.11.  Since draft-ietf-quic-tls-17

   *  Endpoints discard initial keys as soon as handshake keys are
      available (#1951, #2045)

   *  Use of ALPN or equivalent is mandatory (#2263, #2284)

C.12.  Since draft-ietf-quic-tls-14

   *  Update the salt used for Initial secrets (#1970)

   *  Clarify that TLS_AES_128_CCM_8_SHA256 isn't supported (#2019)

   *  Change header protection

      -  Sample from a fixed offset (#1575, #2030)

      -  Cover part of the first byte, including the key phase (#1322,
         #2006)

   *  TLS provides an AEAD and KDF function (#2046)

      -  Clarify that the TLS KDF is used with TLS (#1997)

      -  Change the labels for calculation of QUIC keys (#1845, #1971,
         #1991)

   *  Initial keys are discarded once Handshake keys are available
      (#1951, #2045)




Thomson & Turner        Expires 11 December 2020               [Page 55]

Internet-Draft          Using TLS to Secure QUIC               June 2020


C.13.  Since draft-ietf-quic-tls-13

   *  Updated to TLS 1.3 final (#1660)

C.14.  Since draft-ietf-quic-tls-12

   *  Changes to integration of the TLS handshake (#829, #1018, #1094,
      #1165, #1190, #1233, #1242, #1252, #1450)

      -  The cryptographic handshake uses CRYPTO frames, not stream 0

      -  QUIC packet protection is used in place of TLS record
         protection

      -  Separate QUIC packet number spaces are used for the handshake

      -  Changed Retry to be independent of the cryptographic handshake

      -  Limit the use of HelloRetryRequest to address TLS needs (like
         key shares)

   *  Changed codepoint of TLS extension (#1395, #1402)

C.15.  Since draft-ietf-quic-tls-11

   *  Encrypted packet numbers.

C.16.  Since draft-ietf-quic-tls-10

   *  No significant changes.

C.17.  Since draft-ietf-quic-tls-09

   *  Cleaned up key schedule and updated the salt used for handshake
      packet protection (#1077)

C.18.  Since draft-ietf-quic-tls-08

   *  Specify value for max_early_data_size to enable 0-RTT (#942)

   *  Update key derivation function (#1003, #1004)

C.19.  Since draft-ietf-quic-tls-07

   *  Handshake errors can be reported with CONNECTION_CLOSE (#608,
      #891)





Thomson & Turner        Expires 11 December 2020               [Page 56]

Internet-Draft          Using TLS to Secure QUIC               June 2020


C.20.  Since draft-ietf-quic-tls-05

   No significant changes.

C.21.  Since draft-ietf-quic-tls-04

   *  Update labels used in HKDF-Expand-Label to match TLS 1.3 (#642)

C.22.  Since draft-ietf-quic-tls-03

   No significant changes.

C.23.  Since draft-ietf-quic-tls-02

   *  Updates to match changes in transport draft

C.24.  Since draft-ietf-quic-tls-01

   *  Use TLS alerts to signal TLS errors (#272, #374)

   *  Require ClientHello to fit in a single packet (#338)

   *  The second client handshake flight is now sent in the clear (#262,
      #337)

   *  The QUIC header is included as AEAD Associated Data (#226, #243,
      #302)

   *  Add interface necessary for client address validation (#275)

   *  Define peer authentication (#140)

   *  Require at least TLS 1.3 (#138)

   *  Define transport parameters as a TLS extension (#122)

   *  Define handling for protected packets before the handshake
      completes (#39)

   *  Decouple QUIC version and ALPN (#12)

C.25.  Since draft-ietf-quic-tls-00

   *  Changed bit used to signal key phase

   *  Updated key phase markings during the handshake

   *  Added TLS interface requirements section



Thomson & Turner        Expires 11 December 2020               [Page 57]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   *  Moved to use of TLS exporters for key derivation

   *  Moved TLS error code definitions into this document

C.26.  Since draft-thomson-quic-tls-01

   *  Adopted as base for draft-ietf-quic-tls

   *  Updated authors/editors list

   *  Added status note

Contributors

   The IETF QUIC Working Group received an enormous amount of support
   from many people.  The following people provided substantive
   contributions to this document:

   *  Adam Langley

   *  Alessandro Ghedini

   *  Christian Huitema

   *  Christopher Wood

   *  David Schinazi

   *  Dragana Damjanovic

   *  Eric Rescorla

   *  Felix Guenther

   *  Ian Swett

   *  Jana Iyengar

   *  奥 一穂 (Kazuho Oku)

   *  Marten Seemann

   *  Martin Duke

   *  Mike Bishop

   *  Mikkel Fahnøe Jørgensen




Thomson & Turner        Expires 11 December 2020               [Page 58]

Internet-Draft          Using TLS to Secure QUIC               June 2020


   *  Nick Banks

   *  Nick Harper

   *  Roberto Peon

   *  Rui Paulo

   *  Ryan Hamilton

   *  Victor Vasiliev

Authors' Addresses

   Martin Thomson (editor)
   Mozilla

   Email: mt@lowentropy.net


   Sean Turner (editor)
   sn3rd

   Email: sean@sn3rd.com



























Thomson & Turner        Expires 11 December 2020               [Page 59]
