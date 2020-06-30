# quic-transport

This module contains all main runtime components for receiving and sending data
via the QUIC protocol.

## Overview

### Streams

Streams allow to transfer data between the client and the server. Multiple
streams can be multiplexed on a single connection.

### Connections

Connections describe the logic connection between 2 peers.

### Endpoints

Endpoints are responsible for receiving and sending UDP packets. They are the
entry-point into the QUIC stack on a host.
