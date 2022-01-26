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

## Modifying the runner.patch

```
git clone git@github.com:marten-seemann/quic-interop-runner.git
cd quic-interop-runner
gco cd62367f7cf98d16854551fdd8ef6a48ad89d53d

cp <s2n-quic_proj_dir>.runner.patch .
git apply --3way runner.patch # apply the current patch
git add . # add the current changes
```

Make changes to the quic-interop-runner repo and run the following command to sync the changes
to s2n-quic.

```
git reset HEAD && git diff > <s2n-quic_proj_dir>/.github/interop/runner.patch && git add .
```

Then in s2n-quic run the following command to test your changes:
```
rm -rf target/quic-interop-runner && rm -rf /var/tmp/testrun && ./scripts/interop/run --client aioquic --test chacha20 -l /var/tmp/testrun
```


