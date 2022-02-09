# quic-transport

This module contains all main runtime components for receiving and sending data
via the QUIC protocol.

This is an internal crate used by [s2n-quic](https://github.com/awslabs/s2n-quic). The API is not currently stable and should not be used directly.

## Overview

### Streams

Streams allow to transfer data between the client and the server. Multiple
streams can be multiplexed on a single connection.

### Connections

Connections describe the logic connection between 2 peers.

### Endpoints

Endpoints are responsible for receiving and sending UDP packets. They are the
entry-point into the QUIC stack on a host.
