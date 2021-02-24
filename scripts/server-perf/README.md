# Perf runner

## Requirements

* Linux

## Generating a flamegraph with a 1GB file

```bash
./scripts/server-perf/run
```

## Generating a flamegraph with a specific file size


first arg is how much data is sent _from_ the server
second arg is how much data is sent _to_ the server
```bash
./scripts/server-perf/run 1MB 2MB
```

