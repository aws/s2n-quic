# TCP Acceptor

By default we bind min(# of vCPUs, 4) -- though configurable by applications
via `with_workers` -- listening sockets to the configured listening address. On
Linux, each socket is configured with `TCP_DEFER_ACCEPT` to reduce CPU churn
when accepting sockets[^1]. Sockets are also configured with the maximum kernel
backlog supported on the system. On Linux this is normally 4096 (this is not
configurable). Each listening socket has a Tokio task which:

[^1]: otherwise we accept, register with epoll, and typically near-immediately
      read the prelude packet + deregister from the acceptor runtime's epoll
      before forwarding to the application, which adds a bunch of CPU cycles for
      little reason.

1. Calls accept() up to 2x the per-worker stream queue capacity from the
   listening socket.
   - Enqueues into stream queue (capacity is the application configured backlog
     (default `SOMAXCONN`) divided by with_workers). Retains more recent
     sockets if we overflow.
   - See fresh.rs.
2. Assigns newly accepted streams into "worker slots". On overflow, we choose
   whether to evict an existing worker or keep the new stream based on the
   estimated sojourn time of existing worker slots. This is clamped to
   between 1 and 5 seconds (so slots always retain a stream for at least 1
   second, and evict after at most 5).
   - See manager.rs's `next_worker`.
3. Each worker slot is then polled to possibly make progress. Workers attempt
   to read a prelude packet, then derive stream credentials, and enqueue the
   stream for sending to the application.

There are two "behaviors" which support different modes of accepting sockets.
The DefaultBehavior is for same-process accepting. SocketBehavior sends
accepted streams over a Unix domain socket to a different process alongside the
derived credentials for the stream.

## What is checked for incoming streams

The incoming stream's prelude packet is used to derive credentials. If
credentials are not available (UnknownPathSecret), an UnknownPathSecret secret
control packet is encoded and written into the stream. This will bubble out as
either UnknownPathSecret error or a Send error (if encoding the packet fails).

Replay detection is not performed within the acceptor for DefaultBehavior. This
is deferred to when the application attempts to decrypt the first packet (which
will actually be the InitialPacket, typically carrying an empty payload, but
not guaranteed). The InitialPacket is left in the stream buffer for the first
application read() call. For SocketBehavior, the replay detection is performed,
as it cannot be deferred to the other process (which lacks access to the path
secret map).

Replay detection errors are sent back to the client process over UDP (to the
client handshake address). The stream socket itself is closed with no error
sent over the TCP stream in all cases, typically as a ConnectionReset.

## TLS support

Both modes support detecting a TLS client hello as part of receiving the
prelude packet. SocketBehavior will reject such streams, but DefaultBehavior
accepts them and forwards them into a separate runtime for TLS handshaking
if TLS handshakes were enabled at construction time. Once handshaking
completes, the stream is forwarded into the same application accept queue.

