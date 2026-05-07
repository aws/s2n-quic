# Stream2 Design: Pipeline-Based Streams

## Context and Motivation

We have a working bespoke stream implementation in the wheel-demo that uses the reliable datagram pipeline to initialize flows and send data. The pipeline module handles all the hard parts: retransmission, ACKs, congestion control, and reliable delivery. This is a major architectural shift from the existing streams in the stream/ directory, which implement packet-level reliability with dedicated workers.

The next step is to build a mostly-API-compatible version of the application interface for existing streams, but building on top of the reliable datagram pipeline. This means the new responsibility of streams is much narrower: fragmentation, reassembly, and flow control. Everything else is delegated to the pipeline.

We're calling this stream2 and keeping it as a completely separate implementation in a new module. No refactoring of existing code. The goal is to get something working in production today that can handle hundreds of thousands of concurrent flows sending to hundreds of peers, with a mix of tiny single-packet streams and massive multi-gigabyte streams.

## Why stream2 Exists

The reliable datagram pipeline fundamentally changes what a stream needs to do. In the old world, streams had to manage packet numbers, track acknowledgments, handle retransmissions, and coordinate across multiple worker threads. This resulted in thousands of lines of complex state machine code.

With the pipeline handling reliability at the datagram level, streams become much simpler. The stream just needs to break application data into MTU-sized datagrams, hand them to the pipeline, wait for completion notifications, reassemble datagrams back into an ordered byte stream on the receiver, and manage flow control so neither side buffers too much data.

This is so much simpler that building a new implementation is faster than trying to refactor the existing one. Plus we can run both in parallel during the transition.

## Module Organization

Everything lives in dc/s2n-quic-dc/src/stream2/. We'll have separate files for send, recv, flow control logic, and the runtime abstraction. The public API will be exported from src/stream2.rs.

Both halves of the stream should be mostly independent once opened, similar to what we have now. They'll be different structs with their own concerns. The Writer doesn't need to know about reassembly, and the Reader doesn't need to know about fragmentation.

## Application Endpoint Handle

The application interface will be built around an endpoint handle that encapsulates the pipeline and all necessary infrastructure. Applications shouldn't have to pass around pipeline references or manage low-level details.

A client and server in the same process share the same pipeline. The endpoint configuration takes a client PSK provider (required) and an optional server PSK provider (if the process wants to accept incoming connections). This ensures symmetric send and receive capabilities on a single pipeline instance.

The endpoint handle provides connect() for clients and acceptor registration for servers. Both server and client eventually return Writer and Reader pairs.

## Flow Initialization

The existing streams have a connect method that does the handshake. We'll keep that pattern. On the client side, connect() will be called on the endpoint handle with a peer address and acceptor ID. It returns a Writer and Reader pair.

The key optimization is lazy flow initialization. When connect() is called, we allocate the local queues using the flow queue allocator, but we don't send the FlowInit packet yet. The Writer and Reader are created and returned immediately.

When the application does its first write, that's when we send the FlowInit packet. We can even include early data in that first packet. This saves a round trip for short-lived flows.

The FlowInit packet has routing info that tells the server which acceptor to deliver it to, what the client's queue ID is, and what stream ID we're using. The server receives this through the acceptor registry, which spawns a handler task that creates its own Reader and Writer halves.

The server sends back a FlowControl packet to indicate that the flow has been accepted by the application. This may or may not contain a MAX_DATA frame yet which would set the next window of data that can be sent from the client. When the client Writer receives this acknowledgment, it marks the flow as established and can proceed with sending data packets, assuming the MAX_DATA value provides enough credits.

If the Writer needs to send more data before the flow is established, it blocks waiting for that FlowControl acknowledgment. Similarly, if it runs out of remote flow control budget later, it blocks waiting for MAX_DATA frames.

## Fragmentation and Writer Flow

The Writer's job is to convert the application's byte stream into MTU-sized reliable datagrams. The MTU comes from the path secret entry and doesn't change during the stream's lifetime. This is targeting data center traffic where path MTU doesn't vary.

The Writer maintains several pieces of state. It tracks the next byte offset to send, which starts at zero and increments as we send data. It tracks whether the flow is established yet. It tracks the local and remote queue IDs. The remote queue ID comes from the FlowControl acknowledgment.

For flow control, the Writer tracks local budget and remote budget separately. Local budget is about how much data we can have in flight (pending ACKs) before we run out of buffer space. Remote budget is the peer's advertised MAX_DATA limit that tells us how much data they're willing to receive.

When the application calls write, we first check if we have local budget. Local budget is consumed when we send datagrams and freed when we receive completion notifications from the pipeline. If we're out of local budget, we block polling the completion queue until some datagrams are acknowledged and we free up space.

Next we check remote budget. If the next offset we want to send would exceed the remote budget, we block waiting for a MAX_DATA frame from the peer to increase the limit.

Once both budgets are available, we fragment the application's data. We pull data from the application's buffer into MTU-sized ByteVec chunks. Each chunk becomes a PartialDatagram with FlowData routing that includes the stream ID, queue pair, and byte offset.

We submit these datagrams to the pipeline by building batches and sending them to the wheel via wheel_tx. Each datagram gets a completion sender attached so the pipeline can notify us when it's been reliably delivered.

The Writer also needs to handle FIN. When the application calls write_all_from_fin, the last datagram in that write gets the FIN flag set. The FIN is just metadata in the routing info, it doesn't affect the payload.

The completion queue should be polled continuously. Each completion notification represents a datagram that was acknowledged by the receiver, so we can decrement our local budget used counter and allow more data to be sent.

## Local Flow Control Details

For the MVP we're using a fixed local budget per stream. The target is roughly 6-7 MiB, which allows a single stream to reach the 25 Gbps target with a 2ms RTT (BDP = 25 Gbps × 2ms = 6.25 MiB). This is tunable and we can adjust based on actual RTT measurements in production. This is the maximum amount of unacknowledged data we'll allow in flight. With potentially hundreds of thousands of concurrent streams, this could consume significant memory, but the target environment has 512 GiB available and we're optimizing for throughput.

The reason we need local flow control at all is memory management. Until datagrams are acknowledged, we have to retain them for potential retransmission. The pipeline handles the actual retransmission, but it needs to keep the payload bytes around. By limiting how much unacknowledged data we allow per stream, we bound the memory footprint.

The proper solution here is auto-tuning based on the completion queue delivery rate. If completions are coming back quickly, we can increase the budget to keep the pipe full. If completions are slow, we should reduce the budget to avoid buffering data that isn't contributing to throughput.

This is similar in spirit to the recv_budget module in the existing streams, but instead of tracking application drain rate we'd track completion queue rate. We're punting on this for the MVP because it adds complexity and we need to ship today, but there's a clear TODO to implement it.

## Reassembly and Reader Flow

The Reader's job is to convert out-of-order datagrams back into an ordered byte stream. Datagrams can arrive out of order because the pipeline doesn't preserve ordering, only reliability.

The Reader receives datagrams through its flow queue stream channel. Each message is either data with an offset and payload, a flow validated notification, or a reset. For data messages, we look at the offset.

If the offset matches our next expected offset, we can deliver the data immediately to the application. We advance the next expected offset by the payload length and check if we have any buffered out-of-order data that's now contiguous. If so, we deliver that too.

If the offset doesn't match, it's either a duplicate or out-of-order. We ignore duplicates. For out-of-order data, we buffer it in a data structure keyed by offset. When later data arrives that fills the gap, we can drain the map and deliver contiguous chunks.

This gives the same guarantees as TCP and QUIC at the stream level. The application sees an ordered byte stream with no gaps or reordering.

The Reader also handles FIN. When we receive a datagram with the FIN flag, we mark that we've seen the end of the stream. Once we've delivered all data up to and including the FIN offset, further reads return EOF.

## Remote Flow Control and MAX_DATA

The Reader needs to tell the Writer how much data it's willing to receive. This is remote flow control from the Reader's perspective, local flow control from the Writer's perspective.

The Reader maintains a MAX_DATA window that it advertises to the sender. For the MVP this is a fixed window matching the sender's local budget (6-7 MiB). The Reader tracks how many bytes the application has consumed and sends MAX_DATA updates to keep the sender from stalling.

MAX_DATA frames are sent via FlowControl routing. We encode the frame using the QUIC MAX_DATA frame format from s2n-quic-core, put it in the datagram's control data buffer, and send it through the wheel.

We send MAX_DATA at flow establishment time to give the sender an initial window. After that, we send updates every time the application consumes roughly half the window worth of data. This frequency is a balance: we need to update often enough that the sender doesn't stall waiting for budget, but not so often that we spam the network with control packets.

The proper solution is auto-tuning based on delivery rate. The goal is to buffer just enough data to satisfy application read demand and never more. If the application is reading slowly, we should shrink the window to avoid buffering data that won't be consumed soon. If it's reading quickly, we should grow the window to avoid starving the sender.

This is similar to recv_budget but measuring the rate at which the application drains data from our reassembly buffer, not the rate at which we can deliver completions. Again, punting for MVP with a clear TODO.

## Buffer Management and API

We're going all-in on the buffer trait interfaces from s2n-quic-core. The Writer uses buffer::reader::storage::Infallible for application input, and the Reader uses buffer::writer::Storage for application output. This means we support both raw slices and Bytes/BytesMut, giving applications flexibility.

The Writer API is straightforward. write_from takes a mutable reference to a reader buffer and returns how many bytes were written. It blocks if we're out of flow control budget, either local or remote. write_all_from writes everything in the buffer. write_all_from_fin writes everything and sends FIN. There's also a shutdown method to close the stream gracefully.

The Reader API is similar. read_into takes a mutable reference to a writer buffer and returns how many bytes were read. There's also a poll_read_into for zero-copy patterns that need more control.

We're not exposing poll_ready or separate flush operations. The write methods handle all the blocking and backpressure internally. This keeps the API simple and matches the existing stream API.

## Runtime Abstraction and Testing

The pipeline currently assumes busy poll workers. To make this compatible with tokio and testable with bach, we need a generic spawner abstraction.

The spawner trait needs to know how many workers the runtime has and be able to spawn tasks onto specific workers. This is important because we use Rc and RefCell for same-worker communication to avoid atomic overhead. Cross-worker communication uses Arc and Mutex where necessary.

The spawner has methods for getting the worker count, spawning a task on a specific worker for non-Send futures, and spawning a task on any worker for Send futures. We'll have implementations for busy-poll, tokio, and bach.

Getting bach working is part of the critical path for today because we need to be able to write deterministic tests for flow control, reassembly, and multi-stream behavior. The wheel-demo is great for integration testing but doesn't give us the fine-grained control we need for correctness testing.

## Integration Points

Stream2 integrates with the pipeline infrastructure in several ways. It uses the flow queue allocator to create queues for each stream. It submits datagrams via the wheel input channel. It receives completion notifications via the datagram completion receiver. And it receives incoming data via the flow queue stream channel.

On the server side, stream2 integrates with the acceptor registry. We register a stream2-specific acceptor with a known acceptor ID. When a FlowInit arrives with that acceptor ID, the registry routes it to our acceptor, which spawns a handler task that creates the Reader and Writer halves and pushes them to an accept queue for the application.

The path secret entry provides the MTU and cryptographic material. The wheel handles pacing and sends to sockets. The pipeline handles retransmission and ACKs. Stream2 just sits on top of all this and presents a simple byte stream abstraction.

## Performance Targets and Constraints

The target environment is high-end data center hardware: 64 cores, 512 GiB memory, 25 Gbps network. We're optimizing for throughput while minimizing buffering.

We need to support hundreds of thousands of concurrent flows. Some flows are tiny, just a single packet. Others are massive, potentially gigabytes. The fixed 6-7 MiB windows mean we could theoretically use 6.5 MiB × 100k = 650 GiB if every stream hits its limit, but in practice most streams are short-lived or idle, so actual usage will be much lower.

The key insight is that we want to minimize buffering without sacrificing throughput. We don't want to pull in a bunch of work just to have it sit in buffers. We want data flowing through the system at line rate with minimal resident memory.

This is why auto-tuning is important for production long-term, even if we punt on it for the MVP. A fixed window is either too small (hurts throughput) or too large (wastes memory). An adaptive window can find the sweet spot.

## What We're Not Doing

We're not implementing per-peer flow control, only per-stream. QUIC has both but we don't need that complexity.

We're not implementing sophisticated backpressure APIs like poll_ready or try_write. The write methods block until budget is available. This is simple and matches existing stream behavior.

We're not implementing dynamic MTU updates. The MTU is fixed at stream creation time based on the path secret entry.

We're not worrying about migration or backward compatibility with existing streams. This is a separate implementation that will eventually replace the old one, but for now they coexist.

## Migration and Validation

The plan is to ship stream2 in production alongside the existing streams. We'll incrementally migrate workloads and validate that behavior matches. Once stream2 is proven stable under production load, we can deprecate the old stream implementation.

Testing will use bach for deterministic unit and integration tests once the spawner abstraction is ready. We'll also use the wheel-demo as a integration test bed, replacing the bespoke stream implementation there with stream2.

The key validation is memory usage under load. With hundreds of thousands of streams, we need to confirm that the fixed windows don't cause memory exhaustion, and we need to measure what percentage of streams actually hit their buffer limits so we can tune the auto-tuning heuristics later.
