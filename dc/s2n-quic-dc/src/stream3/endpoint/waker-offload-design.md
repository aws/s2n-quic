# Waker Offload Design

## Problem

The recv dispatch workers run on busy-poll threads and process packets at line rate. After decrypting and dispatching each frame to its destination flow queue, the dispatch thread must wake the consumer task (a tokio task on a separate thread) so it can process the new data.

Every wake from the busy-poll thread is a cross-thread wake into tokio. This means an atomic push to the target worker's injection queue plus a futex/eventfd syscall if the tokio worker is parked. Measurements show this costs p50=3-4us and p99=55-97us per wake.

With 13 frames per packet and ~45k packets/sec, the dispatch thread performs ~250k wakes/sec per queue type (data and flow_control). The aggregate wake cost dominates dispatch time: dispatch is p50=90us, which is entirely attributable to wake syscalls.

At 10k concurrent flows the wakers are mostly distinct (each flow has its own parked reader task), so per-packet deduplication provides little benefit. At 1M flows the situation is the same. The fundamental issue is that the busy-poll thread is blocking on syscalls that are not part of its core responsibility.

## Requirements

The solution must be generic over the spawn mechanism, matching the existing pattern where pipeline components are composed via channel traits rather than hard-coded to a specific executor. The dispatch thread should produce wakers through a channel, and a separate consumer should drain that channel and invoke the wakes. The consumer's drain budget should be configurable through the existing `Budgets` structure.

## Options Considered

### Option A: Per-dispatch-worker mutex-guarded vec with a dedicated waker thread

Each dispatch worker gets a dedicated slot in a shared vec of mutexes. The slot holds a `Vec<Waker>`. The dispatch worker pushes wakers into its own slot (contention is only with the waker thread, never with other dispatch workers). A single dedicated waker thread loops through all slots, swaps each vec with a local empty vec, and drains the wakers by calling wake on each.

The dispatch worker's critical section is a vec push (amortized constant time, ~5ns). The waker thread's critical section is a three-word swap (~3ns). The vec grows to steady state and never allocates again. There is no cross-dispatch-worker contention because each worker has its own mutex.

The waker thread spins when there is work and yields (sched_yield or spin loop hint) when all slots are empty on a full pass. This burns one core but matches the busy-poll philosophy already used for recv and send.

### Option B: Lock-free SPSC ring per dispatch worker

Each dispatch worker gets a fixed-capacity SPSC ring buffer. A dedicated thread drains all rings. This avoids mutex overhead entirely but introduces the fixed-capacity constraint: if the waker thread falls behind, the ring fills and the dispatch thread must either drop wakes (unsafe — missed notifications) or block (defeats the purpose).

### Option C: Per-packet dedup then channel send

Accumulate wakers during the frame loop within a single packet, deduplicate using `will_wake()`, then send only unique wakers to the consumer. This reduces wake volume when multiple frames in a packet target the same flow. However, at high flow counts (10k-1M), frames within a packet are overwhelmingly destined for distinct flows, making dedup ineffective. This option does not solve the core problem at scale.

### Option D: No wake from recv path; consumer tasks poll on timer

Reader tasks would periodically poll their queue rather than being woken. This eliminates wake cost entirely but adds latency equal to the poll interval. For RPC workloads where tail latency matters, even a 100us poll interval is unacceptable.

## Recommendation

Option A. The mutex-guarded vec with a dedicated waker thread is the simplest design that fully removes wake latency from the dispatch path. It has no capacity constraints (unlike the SPSC ring), no contention between dispatch workers, and trivial steady-state allocation behavior.

The implementation fits naturally into the existing pipeline architecture. The dispatch worker already receives its dependencies through the `RecvDispatchParts` struct. Adding a waker sink (a trait that accepts wakers with a push-like interface) keeps the dispatch code generic over the wake strategy. The waker thread is spawned alongside the other worker threads via the same spawner mechanism.

The drain budget on the waker consumer controls how many wakes are fired per iteration before yielding, providing the same fairness knob that exists for all other pipeline stages. At steady state with 3 dispatch workers each producing ~250k wakers/sec, the waker thread processes ~750k wakes/sec. At 1-5us per wake syscall, this consumes 0.75-3.75ms per second of wall time per wake — well within a single core's capacity.

If profiling later shows the single waker thread becomes a bottleneck (e.g., with many dispatch workers or when tokio workers are frequently parked), the design shards naturally: one waker thread per N dispatch workers, with no architectural change needed since each slot is already independent.
