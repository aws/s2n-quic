# Perf runner

## Requirements

* Linux

## Generating a flamegraph with a 1GB file

```bash
./scripts/server-perf/run
```

## Generating a flamegraph with a specific file size


first arg is how much data should be downloaded from server
second arg is how much data should be uploaded to server
```bash
./scripts/server-perf/run 1MB 2MB
```

