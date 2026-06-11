# Review: `reader-flow` Branch — Recv-Credit Flow Control

## Status

**Do not ship in its current state.** This branch wires the endpoint-wide recv credit pool into the
`Reader`'s flow-control window. The mechanism is sound in outline, but it leaks recv credit on
essentially every completed stream and can wedge a server stream into a permanent deadlock once the
pool drains. Those two defects together explain the panics and hangs observed under production
churn. The full test suite passes, which is misleading: there is no test that asserts pool credit
is conserved across a complete stream lifecycle, and that is exactly the invariant being violated.

This document records the findings from an adversarial swarm review (six parallel finders, each
finding independently verified by two skeptics) plus an independent hand-trace of the credit
arithmetic. It is ordered by severity and ends with a fix plan, configuration recommendations, and
the tests that should lock the fixes down.

## How the branch is supposed to work

The branch adds an endpoint-wide recv credit `Pool`, symmetric to the pre-existing send pool used
by the `Writer`. The intent is to bound the *aggregate* advertised receive window across all streams
on an endpoint, solving the incast problem described in `endpoint-flow-control.md`: rather than each
stream independently advertising a multi-megabyte window, every window extension must first draw
credit from a shared pool whose capacity is the real endpoint-wide receive budget.

The flow has two halves that must balance:

The **acquire** half lives in `Reader::maybe_send_max_data` (`stream/reader.rs`). When the
application has consumed enough that `consumed` crosses `remote_max_data - window_size/2`, the reader
computes a `delta` needed to restore a full window ahead of the consume point, calls
`recv_credit_pool.poll_acquire(delta)`, and — on a grant — advances `remote_max_data` by the granted
amount and sends a `MAX_DATA` frame to the peer. Unlike the writer, the reader holds no
`pending_credits`; the grant is committed straight into the advertised window.

The **release** half lives in dispatch (`endpoint/dispatch.rs`) and the queue slot
(`queue/slot.rs`). As inbound stream data is buffered, `StreamState::observe_offset(end)` tracks the
highest end offset ever seen for the binding and returns how many *new* bytes exceed a per-stream
"unbacked initial window" (`initial_window_remaining`, seeded from `local_recv_max_data` at bind).
Dispatch releases exactly those bytes back to the pool.

For the pool's `available` counter to be conserved, the credit a reader debits by extending its
window must eventually be released — either as bytes arrive, or when the stream terminates and the
unused tail of the window is given back. The branch implements the first path but not the second.

## The core defect: advertised-but-unfilled window is never reclaimed

A reader debits the pool every time it extends its advertised window, but the only credit ever
returned to the pool is for bytes that *physically arrive*. The gap between the window a reader has
advertised and the bytes that actually showed up — `remote_max_data - max_received_offset` — is
debited at acquire time and never released by anyone.

Tracing a single stream end to end makes this concrete. Take a server reader, which boots at
`remote_max_data = 0` and must acquire its entire window from the pool. Suppose it advertises a
3.75 MiB window, the peer sends 1 MiB and then FIN, and the application drains to EOF. The reader
acquired roughly 3.75 MiB of credit from the pool over the stream's life. Dispatch released 1 MiB as
those bytes arrived. The remaining ~2.75 MiB — the window the peer was authorized to use but never
did — stays debited forever. When the reader reaches `Status::Complete`, nothing releases it; when
the `Reader` drops, `ReaderAllocPtr::drop` goes through `slot.abandon()` and releases only an
*unobserved distributor grant* (a grant delivered to the slot but never consumed via
`poll_granted`), which is a different and much smaller quantity.

The drop path's own comment admits the gap. It walks through the reasoning that a dropped reader has
"nothing to release," then corrects itself — "Wait — that reasoning has a hole" — and adds a release
for unobserved grants. But the hole it patches is the unobserved-grant case, not the
committed-but-unfilled advertised window, which is the actual and far larger leak. The FIN
short-circuit in `maybe_send_max_data` has the same blind spot: once `remote_max_data >= final_size`
it returns early, releasing at most a prior unobserved grant and never the trailing window.

The consequence compounds with churn. Every stream that completes or resets without exactly filling
its advertised window permanently debits the pool by approximately `window_size - bytes_received`.
After enough streams the pool's `available` reaches and passes zero. From that point `poll_acquire`
parks every new reader indefinitely, and the distributor has nothing to grant, because the only
thing that releases credit — inbound data arriving — requires window that the parked readers can no
longer advertise. The endpoint's receive path quietly stops making progress.

One clarification worth recording, because the swarm initially disagreed on it: the `Granted(n)`
branch in `ReaderAllocPtr::drop` is **not** the leak, and the fix should not be framed around the
slot's `granted` field. That branch correctly handles a distributor grant that landed on the slot
while the reader was unparked and was never observed. It is a real, necessary release. The leak is
structural and lives entirely outside that accounting: the gap between `remote_max_data` and the
bytes received is never represented in the slot at all.

### Client versus server are not symmetric

The hand-trace surfaced a wrinkle the leak alone obscures. The client and server bootstrap
differently, and the difference matters for any conservation fix.

A **client** reader starts with `remote_max_data = local_recv_max_data` — an *unbacked* free window
it never acquired from the pool — and the queue slot suppresses releasing the first
`initial_window` bytes as they arrive. The unbacked starting window and the suppressed initial
release cancel, so the client's books would balance if the trailing window were released on
terminal.

A **server** reader starts with `remote_max_data = 0` and acquires its *entire* window from the
pool, including the first `initial_window` bytes. But the queue slot still suppresses releasing
those first `initial_window` bytes. So even with a correct `M - R` terminal release, the server
leaks an additional `initial_window` per stream, because it paid the pool for bytes the release path
refuses to refund.

Fixing the leak therefore requires resolving this asymmetry, not just adding a terminal release.
**Decision: model the server's initial window symmetrically with the client's.** The server reader
will start with `remote_max_data = initial_window` as a small *unbacked* window, identical to the
client, rather than `remote_max_data = 0` with an unbounded bootstrap. With both roles starting from
an equal unbacked window and the slot suppressing the same `initial_window` bytes, the accounting is
identical on both sides: the only quantity that must be released on terminal is `remote_max_data -
max_received_offset`. This choice also closes the unbounded-bootstrap defect described below, since
the server now enforces a window from the first byte.

The behavioral cost is that a server's peer may only send `initial_window` bytes before the server's
first `MAX_DATA` arrives, where today it could send without bound until validation. With a small
initial window this is the correct, safe bootstrap and matches how the client already behaves.

## Findings

The findings below are the distinct, confirmed defects. Several of the swarm's raw findings
collapsed into the core leak above, observed from different entry points (the FIN path, the drop
path, the server bootstrap); they are one fix in one area and are not repeated as separate items.

### Finding 1 — Recv credit leak on stream termination (Critical)

The advertised-but-unfilled window, `remote_max_data - max_received_offset`, is debited from the
pool at acquire time and never released on completion, reset, or drop. Under churn the pool drains
to zero and all subsequent window extensions park forever.

Locations: `stream/reader.rs` `maybe_send_max_data` (acquire and commit of `remote_max_data`, the
FIN short-circuit) and `ReaderAllocPtr::drop`; `queue/slot.rs` `observe_offset` (the release side
that only ever returns arrived bytes). This is the primary defect and the root cause of the reported
production failures.

### Finding 2 — Server bootstrap deadlock when the pool is drained (Critical)

A server reader starts at `remote_max_data = 0` and cannot advertise any window until it acquires
one from the pool. If the pool is exhausted, `poll_acquire` returns `Pending`, the reader returns
without sending `MAX_DATA`, the peer cannot send, no release fires, and the parked reader is never
granted — a mutual-starvation deadlock. Finding 1 guarantees the pool eventually drains, so this is
the terminal state of the leak rather than an independent edge case, but a transiently saturated
pool can also trigger it on its own.

Location: `stream/reader.rs` server constructor (`remote_max_data = VarInt::ZERO`) and the
`poll_acquire` → `Pending` path in `maybe_send_max_data`. The symmetric-unbacked-window decision
above resolves this directly: a server that starts with a small unbacked window can always advertise
enough for the peer to make initial progress and drive the first release, even against an empty
pool.

### Finding 3 — Unbounded buffering during server bootstrap (High)

While a server reader is in bootstrap (`remote_max_data == 0`), the receive-window enforcement in
`poll_stream_rx` is bypassed (`if self.remote_max_data != VarInt::ZERO && ...`), so a peer may push
data at any offset and the reader will buffer it. Nothing else bounds this today. Worse, those
never-acquired bootstrap bytes are still `release()`d into the pool as they arrive once past the
suppressed initial window, which corrupts pool conservation in the opposite direction — releasing
credit that was never acquired.

Location: `stream/reader.rs` `poll_stream_rx` window check. Removing the `remote_max_data == 0`
bypass (part of the symmetric-window change) closes the buffering hole; the release side must also
be made consistent so bootstrap bytes covered by the unbacked window are never released.

### Finding 4 — `abandon()` reads `granted` with only indirect ordering documentation (Low)

In `credit/slot.rs`, `grant()` writes `*self.granted.get()` and then performs a `Release` CAS on the
refcount; `abandon()` performs an `Acquire`-on-failure CAS and then reads `*self.granted.get()`. The
release/acquire pairing on the refcount does establish the necessary happens-before for the field
read, so this is sound — but the synchronization is indirect and undocumented at the read site, and
the entire pool's correctness depends on it. This is a documentation and test-coverage gap, not a
known miscompile.

Location: `credit/slot.rs` `grant()` and `abandon()`. The fix is a comment at the read site
explaining the pairing, plus a loom test exercising the concurrent grant/abandon race.

## Explicitly not pursued

`maybe_send_max_data` computes an intermediate `target_max_data = consumed + window_size` with
non-saturating addition while every neighboring computation saturates. In principle a debug build
panics on overflow. In practice these quantities are VarInt-bounded and will not approach `2^62`
under any real workload, so per the branch author this is out of scope and will not be changed.

The swarm also raised and then rejected several candidates after verification: a double-park panic on
re-poll (refuted — `poll_granted` returns `Pending` for a linked slot and `maybe_send_max_data`
early-returns before `prepare_park`, and the existing
`maybe_send_max_data_re_polls_without_double_parking` test covers it); divergence of
`flush_watermark` and `max_received_offset` on rebind (already fixed by `StreamState::clear`
resetting both); oscillating `MAX_DATA` sends (guarded by the `delta == 0` early return); and the
`QueueMsg`-init path discarding `release_bytes` (the discarded value is zero because the bind pushes
no data).

## Fix plan

The fixes are ordered so each is provable before the next builds on it.

First, **reproduce the leak as a failing test** before changing any logic. A bach sim that opens a
server stream against a finite recv pool, sends a fraction of the advertised window plus FIN, drains
to EOF, drops the reader, and asserts that `pool.debug_available()` has returned to capacity. It will
fail today by roughly the unfilled window, proving the defect.

Second, **make server bootstrap symmetric**. Change the server constructor to start
`remote_max_data = initial_window` as a small unbacked window and remove the `remote_max_data == 0`
enforcement bypass in `poll_stream_rx`, so the window is enforced from the first byte. Ensure the
release side suppresses exactly the unbacked `initial_window` bytes for both roles, so neither leaks
nor over-releases the bootstrap region. This resolves Findings 2 and 3 and removes the asymmetry that
would otherwise defeat the terminal release.

Third, **release the trailing window on every terminal path**. On clean completion (the FIN
short-circuit) and on drop (`ReaderAllocPtr::drop`), compute the unfilled window as
`remote_max_data - max_received_offset` and release it to the pool, guarding so the completion-path
release and the drop-path release are mutually exclusive (for example by zeroing the tracked window
after releasing, or a `released_trailing` flag). The reader needs the highest received offset at
drop time; it can be threaded into the reader's `Inner` from the slot or derived from the reassembler.

Fourth, **document and loom-test the slot ordering** (Finding 4): a comment at the `granted` read in
`abandon()` explaining the release/acquire pairing, and a loom test racing `grant()` against
`abandon()` that asserts no torn or stale read and exact pool conservation.

After the fixes, the reproduction test from step one should pass, and it should be extended to run
many streams sequentially and assert the pool never depletes.

## Configuration recommendations

Pool sizing is moot until Finding 1 is fixed — any pool drains within a handful of streams while the
leak exists — but the target values are recorded here so they can land alongside the fix.

The **recv pool capacity** is the endpoint-wide ceiling on advertised-but-unfilled receive window
across all streams. It should be sized to the bandwidth-delay product of the endpoint's default
throughput, around 8 MiB for the default target (a touch more is fine). Using
`8 × throughput_gbps × 125 × rtt_ms / 1000` MiB as a rule of thumb, 150 Mbps at 100 ms RTT gives a
BDP near 1.9 MiB, so an 8–16 MiB pool carries several BDP of headroom. The current 256 MiB default
is both too coarse to expose the leak in testing and too large to fail fast in production; it should
come down to the BDP-scaled value.

The **per-priority `max_single_acquire` cap** in `credit/config.rs` must not fall below one full
`window_size`. A reader extends by a full window in a single acquire, so if the bulk-tier clamp
(`capacity / 64` today) drops under `window_size` at a small pool capacity, window extensions
silently truncate. At an 8 MiB pool this is a real risk; the bulk-tier cap should be floored at
`window_size`. The latency-tier cap (`capacity / 256`) can stay as is to keep a single latency-
sensitive reader from monopolizing the pool.

The **initial unbacked window** is the key tuning change and the one the symmetric-bootstrap fix
depends on. Today it is seeded from `local_recv_max_data`, so the initial window equals the full
window (~3.75 MiB), which is what exposes the unbounded-bootstrap defect. It should become a separate,
much smaller parameter — on the order of 64 KiB — so a fresh stream can make initial progress without
a pool grant while bounding how much an un-windowed peer can buffer. Because `local_recv_max_data`
also drives `window_size`, this needs a dedicated parameter on `ApplicationParams` rather than
reusing `local_recv_max_data`.

The per-stream `window_size` itself (one RTT's pipe, ~3.75 MiB from `local_recv_max_data`) is a
reasonable default and can stay. The **send pool** can be much smaller than the recv pool — it tracks
local pre-transmission frame queuing rather than in-flight bytes — and sub-MiB is typically
sufficient; it should not share the recv pool's 256 MiB default.

## Tests to add

The decisive test is **pool conservation across a full lifecycle**, which is the test whose absence
let the leak pass CI: a server reader against a finite pool sends a fraction of its window plus FIN,
drains to EOF, drops, and asserts `available` returns to capacity; then the same scenario run many
times sequentially asserting the pool never depletes. A companion test covers the **reset and
mid-stream-drop** path, where the reader advertises a window, the peer sends a fraction without FIN,
and the reader is dropped — asserting the trailing window is released. A **bootstrap liveness** test
drains the pool, opens a fresh server stream, and asserts the reader still advertises its small
unbacked window and the sim makes progress rather than deadlocking. A **bootstrap buffering** test
sends well beyond the initial window before any read and asserts buffering stays bounded and no
credit is released for never-acquired bytes. Finally, a **loom test** races `grant()` against
`abandon()` under `--features loom` and asserts no torn `granted` read and exact conservation.
