# UDP Sender Performance Investigation

**Date:** 2026-04-17  
**Context:** Busy poll runtime with dedicated sender threads

## Problem: Wheel Ticker Backlog

The timing wheel ticker was building up ~1 million tick backlog during periods where the channel send blocked. This caused 2-3ms bursts of catch-up work when the wheel finally advanced.

### Root Cause

The original `wheel_ticker` implementation would:
1. Tick the wheel to current time
2. Send due entries through the channel (blocking await)
3. Idle until next work
4. Repeat

If the channel send blocked for ~1 second (receiver busy), the wheel wouldn't advance during that time. On the next iteration, `tick_to(now)` would need to advance through ~1 million ticks (1 second / 1µs granularity), causing 2-3ms of synchronous work.

### Solution

Restructured `wheel_ticker` to interleave wheel advancement with channel sends:

```rust
let mut pending_queue = Queue::new();
loop {
    let now = timer.now();
    let mut queue = ticker.tick_to(now.into());
    pending_queue.append(&mut queue);
    
    if !pending_queue.is_empty() {
        let to_send = core::mem::take(&mut pending_queue);
        let mut send_fut = pin!(tx.send(to_send));
        
        // Poll the send future, ticking the wheel while blocked
        let result = poll_fn(|cx| {
            match send_fut.as_mut().poll(cx) {
                Poll::Ready(result) => Poll::Ready(result),
                Poll::Pending => {
                    // While send is blocked, tick the wheel and accumulate
                    let now = timer.now();
                    let mut new_queue = ticker.tick_to(now.into());
                    pending_queue.append(&mut new_queue);
                    Poll::Pending
                }
            }
        }).await;
    }
    
    idle.idle(&ticker, &mut timer).await;
}
```

**Key insight:** Continue ticking the wheel while the send future is pending. This distributes the work over time rather than letting it accumulate.

### Result

User feedback: "ok i think that improved things quite a bit!"

The wheel ticker now maintains a more even distribution of work, avoiding large bursts of catch-up computation.

## Unresolved: Sporadic 2ms Sender Poll Overhead

### Observations

Sporadic "slow sender poll" warnings appear with 2-3ms poll durations, occurring roughly every few seconds. This represents <0.1% overhead but is concerning for busy poll runtimes where any pause can affect sender thread responsiveness.

### Investigation

Added instrumentation to measure:
- UDP send operation timing
- Completion callback timing  
- Token bucket pacing sleep timing
- Channel recv poll timing

**Result:** None of the instrumentation fired during slow polls.

### Hypothesis

The overhead is occurring in the async state machine polling itself or tokio runtime scheduling, not in application code execution. The 2ms likely represents:
- Async state machine traversal overhead
- Tokio runtime scheduling decisions
- Context switching or other OS-level overhead

### Impact

- Sporadic: ~2ms every few seconds
- Overhead: <0.1% of total runtime
- System performing well overall
- Concern: Any long pauses in busy poll runtime could affect sender threads

### Next Steps

Document and defer. Revisit if:
1. Pauses become more frequent or longer
2. They correlate with observable performance issues
3. We have better tooling to profile async runtime overhead

## Instrumentation Added

For future debugging, the following instrumentation remains in place:

**`socket/send/udp.rs`:**
- Ticker poll duration tracking (line ~331-340)
- Sender poll duration tracking (line ~350-357)
- UDP send timing (line ~224-232)
- Completion timing (line ~234-240)
- Channel recv poll timing (line ~199-207)

**`socket/send/wheel.rs`:**
- `drain_incoming` duration
- Wheel advance duration
- Ticks to advance count

All instrumentation uses `tracing::warn!` with 1ms threshold.
