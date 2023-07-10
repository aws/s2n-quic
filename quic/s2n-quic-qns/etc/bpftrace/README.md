# s2n-quic bpftrace

This directory contains several bpftrace programs that can be used to analyze certain aspects of the s2n-quic implementation. The easiest way to get started is by enabling the `usdt` feature for the `s2n-quic-qns` application:

```bash
$ cargo build --features usdt --bin s2n-quic-qns
```

## generic-offload.bt

This program shows GRO and GSO usage for a transfer. Generally, usage should be high for bulk transfer.

````bash
$ sudo bpftrace -c './target/release/s2n-quic-qns perf server --port 4434 --queue-send-buffer-size 8000000 --multithread' quic/s2n-quic-qns/etc/bpftrace/generic-offload.bt

@gro_count:
[1]                35672 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|

@gro_size:
[32, 64)           34806 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[64, 128)            858 |@                                                   |
[128, 256)             1 |                                                    |
[256, 512)             0 |                                                    |
[512, 1K)              0 |                                                    |
[1K, 2K)               4 |                                                    |
[2K, 4K)               3 |                                                    |

@gso_count:
[0]                   19 |                                                    |
[1]                  599 |                                                    |
[2, 4)              1822 |                                                    |
[4, 8)              8243 |                                                    |
[8, 16)           687324 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|

@gso_size:
[32, 64)              12 |                                                    |
[64, 128)            163 |                                                    |
[128, 256)             2 |                                                    |
[256, 512)            14 |                                                    |
[512, 1K)              0 |                                                    |
[1K, 2K)              19 |                                                    |
[2K, 4K)          697791 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[4K, 8K)               6 |                                                    |
````

## io-latencies.bt

This program shows how long (in nanoseconds) it takes for a packet to be processed by the IO
tasks. This does take into account the time spent in the queue, which generally means the larger the queue
the higher the latency.

### 128k buffer

```
@rx_latencies:
[2K, 4K)           22223 |@@@@@@                                              |
[4K, 8K)           31559 |@@@@@@@@@                                           |
[8K, 16K)          14658 |@@@@                                                |
[16K, 32K)         80708 |@@@@@@@@@@@@@@@@@@@@@@@@                            |
[32K, 64K)         27705 |@@@@@@@@                                            |
[64K, 128K)       170845 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[128K, 256K)       72525 |@@@@@@@@@@@@@@@@@@@@@@                              |
[256K, 512K)         287 |                                                    |


@tx_latencies:
[2K, 4K)               1 |                                                    |
[4K, 8K)              59 |                                                    |
[8K, 16K)            183 |                                                    |
[16K, 32K)        398915 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[32K, 64K)         46878 |@@@@@@                                              |
[64K, 128K)          105 |                                                    |
[128K, 256K)           8 |                                                    |
```

### 8mb buffer

```
@rx_latencies:
[4K, 8K)              32 |                                                    |
[8K, 16K)            296 |                                                    |
[16K, 32K)          2011 |@@@@@@                                              |
[32K, 64K)          1680 |@@@@@                                               |
[64K, 128K)         1784 |@@@@@                                               |
[128K, 256K)        9522 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@                     |
[256K, 512K)       15498 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[512K, 1M)         15323 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@ |
[1M, 2M)             348 |@                                                   |
[2M, 4M)               4 |                                                    |


@tx_latencies:
[8K, 16K)             14 |                                                    |
[16K, 32K)             9 |                                                    |
[32K, 64K)            15 |                                                    |
[64K, 128K)           82 |                                                    |
[128K, 256K)        6008 |                                                    |
[256K, 512K)      211711 |@@@@@@@@@@@@@@@@@@                                  |
[512K, 1M)        606386 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[1M, 2M)          352134 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@                      |
[2M, 4M)             508 |                                                    |
```

## recv-buffers.bt

This program shows the operations performed on the stream receive buffers. `@allocs` is the
number of allocations performed. `@pops` is the size of the popped `Bytes` chunk when reading
the receive buffer from the application. `@writes` is the size of the STREAM frame payload that
gets copied into the receive buffer.

```
@allocs:
[4K, 8K)              18 |                                                    |
[8K, 16K)              0 |                                                    |
[16K, 32K)            12 |                                                    |
[32K, 64K)            24 |                                                    |
[64K, 128K)       388937 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|

@pops:
[1]                    2 |                                                    |
[2, 4)                 2 |                                                    |
[4, 8)                10 |                                                    |
[8, 16)               16 |                                                    |
[16, 32)              29 |                                                    |
[32, 64)              61 |                                                    |
[64, 128)            114 |                                                    |
[128, 256)           228 |                                                    |
[256, 512)           462 |                                                    |
[512, 1K)            938 |                                                    |
[1K, 2K)            1838 |                                                    |
[2K, 4K)            3704 |                                                    |
[4K, 8K)            7379 |@                                                   |
[8K, 16K)          14769 |@@                                                  |
[16K, 32K)         29531 |@@@@                                                |
[32K, 64K)         59044 |@@@@@@@@@                                           |
[64K, 128K)       329909 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|

@writes:
[1]                  218 |                                                    |
[2, 4)               440 |                                                    |
[4, 8)               920 |                                                    |
[8, 16)             1991 |                                                    |
[16, 32)            7924 |                                                    |
[32, 64)           18642 |                                                    |
[64, 128)          14614 |                                                    |
[128, 256)         29213 |                                                    |
[256, 512)         65119 |                                                    |
[512, 1K)         135208 |                                                    |
[1K, 2K)          242183 |@                                                   |
[2K, 4K)         7368225 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
```

## udp-syscall.bt

This program probes the UDP syscalls performed by the IO tasks. `arg_counts` shows how many
`mmsg` packets are passed to a single syscall. `latencies` shows the time it takes (in nanoseconds) to
perform the syscall. `ret_counts` is the number returned from a syscall.

### 128kb buffer

```bash
$ sudo bpftrace -c './target/release/s2n-quic-qns perf server --port 4433 --queue-send-buffer-size 128000 --multithread' quic/s2n-quic-qns/etc/bpftrace/udp-syscall.bt

@rx_arg_counts:
[32, 64)            1215 |                                                    |
[64, 128)         315312 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[128, 256)        234035 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@              |

@rx_latencies:
[512, 1K)         253865 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[1K, 2K)          159559 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@                    |
[2K, 4K)           92526 |@@@@@@@@@@@@@@@@@@                                  |
[4K, 8K)           29469 |@@@@@@                                              |
[8K, 16K)          10716 |@@                                                  |
[16K, 32K)          3934 |                                                    |
[32K, 64K)           479 |                                                    |
[64K, 128K)            7 |                                                    |
[128K, 256K)           7 |                                                    |

@rx_ret_counts:
[0]               267717 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[1]               233216 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@       |
[2, 4)              6523 |@                                                   |
[4, 8)              7546 |@                                                   |
[8, 16)            25710 |@@@@                                                |
[16, 32)            8100 |@                                                   |
[32, 64)            1425 |                                                    |
[64, 128)            325 |                                                    |


@tx_arg_counts:
[1]               856126 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|

@tx_latencies:
[2K, 4K)              95 |                                                    |
[4K, 8K)          770935 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[8K, 16K)          83146 |@@@@@                                               |
[16K, 32K)          1922 |                                                    |
[32K, 64K)            10 |                                                    |
[64K, 128K)            5 |                                                    |
[128K, 256K)          13 |                                                    |

@tx_ret_counts:
[1]               856126 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
```

### 8mb buffer

```bash
$ sudo bpftrace -c './target/release/s2n-quic-qns perf server --port 4434 --queue-send-buffer-size 8000000 --multithread' quic/s2n-quic-qns/etc/bpftrace/udp-syscall.bt

@rx_arg_counts:
[64, 128)         111052 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[128, 256)         23737 |@@@@@@@@@@@                                         |

@rx_latencies:
[512, 1K)          62148 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[1K, 2K)           22085 |@@@@@@@@@@@@@@@@@@                                  |
[2K, 4K)           47729 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@             |
[4K, 8K)            2681 |@@                                                  |
[8K, 16K)            139 |                                                    |
[16K, 32K)             7 |                                                    |

@rx_ret_counts:
[0]                67372 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[1]                67307 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@ |
[2, 4)               108 |                                                    |
[4, 8)                 2 |                                                    |


@tx_arg_counts:
[1]                   15 |                                                    |
[2, 4)                44 |                                                    |
[4, 8)                36 |                                                    |
[8, 16)               39 |                                                    |
[16, 32)            2193 |@@@@@                                               |
[32, 64)           20487 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[64, 128)            947 |@@                                                  |

@tx_latencies:
[2K, 4K)               2 |                                                    |
[4K, 8K)               9 |                                                    |
[8K, 16K)             27 |                                                    |
[16K, 32K)            33 |                                                    |
[32K, 64K)            47 |                                                    |
[64K, 128K)          819 |@@                                                  |
[128K, 256K)        5524 |@@@@@@@@@@@@@@@@@                                   |
[256K, 512K)       16852 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[512K, 1M)           448 |@                                                   |

@tx_ret_counts:
[1]                   15 |                                                    |
[2, 4)                44 |                                                    |
[4, 8)                36 |                                                    |
[8, 16)               39 |                                                    |
[16, 32)            2193 |@@@@@                                               |
[32, 64)           20487 |@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@|
[64, 128)            947 |@@                                                  |
```

