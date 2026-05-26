# Flow Queue Rewrite

This document tracks progress on the clean-slate rewrite of the `flow/queue`
module described in [the design gist][gist].

[gist]: https://gist.github.com/camshaft/5b0d69161c0b3e64dceacb5b4246b692

## Motivation

The existing `flow/queue` implementation is heavily generic (`Pool<S, C, Key,
INITIAL_PAGE_SIZE>`) and encodes assumptions that no longer match the real
usage patterns emerging from the flow-init protocol work (PR #187).  Rather
than continuing to adapt that design, we start fresh with a concrete,
no-generics module that hardcodes `msg::Stream` / `msg::Control` and gives each
side (client / server) purpose-built types.

## New module location

```
dc/s2n-quic-dc/src/queue.rs          -- module root (Error, re-exports)
dc/s2n-quic-dc/src/queue/half.rs     -- Half<T>: push/pop/poll + lifecycle
dc/s2n-quic-dc/src/queue/slot.rs     -- Slot: two halves + atomic binding_id
dc/s2n-quic-dc/src/queue/page_table.rs -- PageTable, State, SenderView, grow
dc/s2n-quic-dc/src/queue/freed.rs    -- FreedSender, FreedBatch (server path)
dc/s2n-quic-dc/src/queue/handle.rs   -- StreamReceiver, ControlReceiver, OnFree
dc/s2n-quic-dc/src/queue/client.rs   -- ClientAllocator, ClientDispatch, ClientFreeList
dc/s2n-quic-dc/src/queue/server.rs   -- ServerDispatch, BindResult
```

The `sync` module (also ported from the `queue-init-simplification` branch)
provides the building blocks:

```
dc/s2n-quic-dc/src/sync/bitset/bitset64.rs    -- BitSet64: 64-bit word bitset
dc/s2n-quic-dc/src/sync/bitset/hierarchical.rs -- HierarchicalBitSet: O(4) ops
dc/s2n-quic-dc/src/sync/free_list.rs          -- FreeList: client peer-side recycling
```

## Design summary

### No generics

The new module drops all type parameters.  Queue entries are always
`intrusive::Entry<msg::Stream>` or `intrusive::Entry<msg::Control>`.

### Slot lifecycle

```
UNALLOCATED  ──try_allocate(binding_id)──▶  ALLOCATED
                                               │
                              open_receivers() │
                                               ▼
                                           OPEN (has receivers)
                                               │
                      StreamReceiver::drop() + │
                     ControlReceiver::drop()   │
                                               ▼
                                           mark_unallocated()
                                               │
                           OnFree::Client  ────┤──── OnFree::Server
                           push_freed(idx) │   │   freed_sender.record(queue_id)
                                           ▼   ▼
                                        UNALLOCATED
```

### Client side

`ClientAllocator` allocates a **local** slot (from `ClientFreeList`, backed by
`HierarchicalBitSet`) and a **peer** `dest_queue_id` (from
`sync::free_list::FreeList`).  It returns an `AllocResult` with both receiver
handles and both IDs in one atomic step.

`ClientDispatch` is the inbound hot path: given a `queue_id` + `binding_id`,
it validates and pushes to the right slot via a `SenderView` (raw-pointer
cache, no lock on lookup).

### Server side

`ServerDispatch::bind_and_send_stream` handles the first packet for a stream:
it CAS-claims an unallocated slot, opens both receiver halves, and returns
`BindResult::NewBinding { stream, control, waker }`.  Subsequent packets go
through `send_stream` / `send_control` (hot path, binding validated inline).

`FreedSender` accumulates freed queue IDs for a peer and emits `FreedBatch`
messages via an unbounded mpsc channel to the endpoint emission task.

### SenderView: lock-free dispatch

`SenderView` caches `(*const Slot, len)` tuples per page.  The cache is
refreshed at most once per page-growth event.  On the hot path (lookup by
index) no lock is held.

### Pinned pages

All `Slot` arrays live in `Pin<Box<[Slot]>>`.  Slots are never moved, so raw
pointers stored in receiver handles remain valid for the `Arc<State>` lifetime.

## Progress

### Foundation

- [x] `sync/bitset/bitset64.rs` — `BitSet64` (ported from branch)
- [x] `sync/bitset/hierarchical.rs` — `HierarchicalBitSet` (ported from branch)
- [x] `sync/free_list.rs` — `FreeList` (ported from branch)
- [x] `queue/half.rs` — `Half<T>`, `AutoWake`, `open_receivers`, `close_receiver`
- [x] `queue/slot.rs` — `Slot` with atomic `binding_id` + lifecycle helpers
- [x] `queue/page_table.rs` — `PageTable`, `SenderView`, `find_page`
- [x] `queue/freed.rs` — `FreedSender`, `FreedBatch`, `freed_batch_channel`
- [x] `queue/handle.rs` — `StreamReceiver`, `ControlReceiver`, `OnFree`, `AllocResult`
- [x] `queue/client.rs` — `ClientAllocator`, `ClientDispatch`, `ClientFreeList`
- [x] `queue/server.rs` — `ServerDispatch`, `BindResult`
- [x] `queue.rs` — module root with `Error` and re-exports
- [x] Module compiles cleanly

### Tests

- [ ] `half.rs` unit tests (push/pop/close lifecycle)
- [ ] `slot.rs` unit tests (try_allocate CAS, mark_unallocated round-trip)
- [ ] `page_table.rs` unit tests (grow, SenderView refresh) — skeleton present
- [ ] `client.rs` unit tests (alloc, dispatch, free-list recycle)
- [ ] `server.rs` unit tests (bind_and_send, concurrent racing binds)
- [ ] `freed.rs` unit tests (record, complete_in_flight, batch emission)
- [ ] Integration / bach simulation test

### Integration (wire-up to existing consumers)

These steps migrate existing code from `flow/queue` to `queue` and remove the
old module.  Tracked separately so this PR can land first and PR #187 can
rebase on top.

- [ ] `endpoint/msg.rs` — update `msg::queue` type aliases
- [ ] `endpoint/dispatch.rs` — swap `flow::queue::Dispatch` for `queue::ClientDispatch` / `queue::ServerDispatch`
- [ ] `endpoint/tasks.rs` — update task wiring for new allocator / dispatch types
- [ ] `endpoint/combinator.rs` — update combinator for new receiver types
- [ ] `acceptor.rs` — update acceptor for new `AllocResult`
- [ ] Stream `reader.rs` / `writer.rs` — use `local_queue_id` / `dest_queue_id` from `AllocResult`
- [ ] Remove `flow/queue/` and `flow/handle.rs` (kept for compat until all consumers migrated)
- [ ] Flatten `flow/` to just `flow.rs` (or remove entirely if nothing remains)
