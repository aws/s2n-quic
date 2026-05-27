## Testing

- Use `cargo nextest` since that includes timeouts
- Use `S2N_LOG={level}` environment variable to change logging level. For debugging individual tests, prefer using `trace` since it will provide the most insight into what's happening. For tests that use `without_tracing` you need to pass `S2N_LOG_FORCED=1`.
- Pipe test output into temporary file. This makes debugging much easier.
- Before fixing a suspected bug, confirm that it's an actual bug by adding a reproduction test. We must prove that a test that was previously failing is now passing with our proposed change.
- Prefer using `bach` for deterministic discrete event testing. Note that test time is simulated wall clock so the actual test time is not the same. The simulated time has no correlation to CPU time. There is a single thread and all tasks are cooperatively scheduled. Data races are not possible in this environment, though it's possible to still have race conditions across await points. Assume bach doesn't have bugs - it's very unlikely that whatever is wrong is bach.
- When debugging issues, ensure investigations include logs. We want to confirm assumed behavior by showing matching log lines.
- When we have visibility gaps, add new tracing log lines for debugging.
- Use subagents to read the log file and summarize what happened and highlight anomalies.

## Documents

- Avoid writing code if possible.
- Only use bullet points sparingly.
- Prefer prose and english descriptions of algorithms and concepts.
- Be precise and concise.
- Don't fabricate metrics or concepts without backing them up.
