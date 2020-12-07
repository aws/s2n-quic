# Interop runner

## Requirements

* docker-compose
* tshark

## Pulling the latest images

```bash
./scripts/interop/run pull
```

## Running with a specific client

```bash
./scripts/interop/run --client quic-go
```

## Running with a specific test

```bash
./scripts/interop/run --client quic-go --test retry
```
