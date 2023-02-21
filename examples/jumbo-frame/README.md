# Jumbo Frame Support
The standard ethernet Maximum Transmission Unit - MTU - is 1500 bytes. Jumbo frames are ethernet frames that support more than 1500 bytes. Jumbo frames will often support approximately 9000 bytes, although this is an implementation specific detail and you should check to see what your network supports.

## why use jumbo frames
Some overheads occur per-packet. For example, UDP packet headers are included on each packet. Using jumbo frames decreases the overhead / payload ratio, so communication is more efficient.

CPU utilization also benefits from jumbo frame usage.

## how to use jumbo frames
Jumbo frames can only be used if they are supported by the network. AWS supports jumbo frames in the specific circumstances listed [here](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/network_mtu.html)

The `tracepath` utility can be used to check if jumbo frames are supported on a local machine.

```console
[ec2-user@ip-1-1-1-1 jumbo-frame]$ tracepath localhost
 1:  localhost                                             0.035ms reached
     Resume: pmtu 65535 hops 1 back 1
```
The `pmtu 65535` indicates that `localhost` has a maximum transmission unit of 65,535 bytes. Networks are unlikely to support an MTU this large, but this confirms that our demo with a 9,001 byte MTU will work properly.

The MTU is a property of the IO provider. This can be configured as shown below.
```rust
    let address: SocketAddr = "127.0.0.1:4433".parse()?;
    let io = s2n_quic::provider::io::Default::builder()
        .with_receive_address(address)?
        // Specify that the endpoint should try to use frames up to 9001 bytes.
        // This is the maximum MTU that most ec2 instances will support.
        // https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/network_mtu.html
        .with_max_mtu(9001)?
        // high throughput jumbo frame scenarios often benefit from larger socket
        // buffers. End-users should experiment to find the optimal configuration
        // for their use case.
        .with_recv_buffer_size(12_000_000)?
        .with_send_buffer_size(12_000_000)?
        .build()?;
```

To run the demo, launch two terminals. In the first terminal, start the server.
```console
[ec2-user@ip-172-31-28-149 jumbo-frame]$ pwd
/home/ec2-user/workspace/s2n-quic/examples/jumbo-frame
[ec2-user@ip-172-31-28-149 jumbo-frame]$ cargo run --bin jumbo_server
   Compiling rustls-provider v0.1.0 (/home/ec2-user/workspace/s2n-quic/examples/jumbo-frame)
    Finished dev [unoptimized + debuginfo] target(s) in 3.23s
     Running `target/debug/jumbo_server`
Listening for a connection

```

In the other terminal, launch the client.
```console
[ec2-user@ip-172-31-28-149 jumbo-frame]$ cargo run --bin jumbo_client
   Compiling rustls-provider v0.1.0 (/home/ec2-user/workspace/s2n-quic/examples/jumbo-frame)
    Finished dev [unoptimized + debuginfo] target(s) in 3.44s
     Running `target/debug/jumbo_client`
MtuUpdated { path_id: 0, mtu: 1200, cause: NewPath }
MtuUpdated { path_id: 0, mtu: 1472, cause: ProbeAcknowledged }
MtuUpdated { path_id: 0, mtu: 5222, cause: ProbeAcknowledged }
MtuUpdated { path_id: 0, mtu: 7097, cause: ProbeAcknowledged }
MtuUpdated { path_id: 0, mtu: 8035, cause: ProbeAcknowledged }
MtuUpdated { path_id: 0, mtu: 8504, cause: ProbeAcknowledged }
MtuUpdated { path_id: 0, mtu: 8738, cause: ProbeAcknowledged }
MtuUpdated { path_id: 0, mtu: 8855, cause: ProbeAcknowledged }
MtuUpdated { path_id: 0, mtu: 8914, cause: ProbeAcknowledged }
MtuUpdated { path_id: 0, mtu: 8943, cause: ProbeAcknowledged }
```

The output confirms that large MTU's (jumbo frames) have been probed.

Note that both endpoints, client and server, must enable jumbo frames for either side to use them. If a client with `max_mtu=9001` tries to probe a server with `max_mtu=1500`, then the server will drop the probes larger than 1500 bytes, and the client will continue to use the 1472 byte MTU that it initially negotiated.

## ensure jumbo frames are being used
The s2n-quic events system has an event for MTU updates. The subscriber defined in `lib.rs` captures this event to output messages like those below.
```
MtuUpdated { path_id: 0, mtu: 8943, cause: ProbeAcknowledged }
```

This event can be used to verify that jumbo frames are being used. Alternatively, tools like `tcpdump` with a packet analysis tool like `wireshark` can confirm that jumbo packets are being sent and received.

## probing strategy
s2n-quic implements Datagram Packetization Layer Path Maximum Transmission Unit Discovery, or DPLPMTUD for short. This is described in [RFC8899](https://www.rfc-editor.org/info/rfc8899). The strategy is as follows. To determine if an `X` byte MTU is supported, send a packet of `X` bytes.
- if it is acked -> supported
- if it is lost  -> not supported

The MTU updates happen in this process
1. 1200 byte MTU - the handshake frames are padded to 1200 bytes. A successful handshake implies at least a 1200 byte MTU. From the quic RFC, this is the minimum MTU that quic supports.
2. 1472 byte MTU - the quic endpoint will send the first probe at `min(1500 - overhead, max_mtu - overhead)`. The 1472 byte probe represents `1500 - UDP_HEADER - IPV4_HEADER`. This value is probed first because it commonly supported.
3. binary search towards `max_mtu` until within a reasonable threshold of `max_mtu`.

The endpoint does not immediately start probing at large values because of poor efficiency in the following scenario.
- a large (approximately 9000) maximum MTU is specified at the endpoint
- the network only supports a small MTU (approximately 1500)
- the connection exchanges a small amount of data

Sending 9000 bytes of application data entails
- 9000 bytes of application data
- 9000 bytes of probe
The overhead of probing would be 50%. Starting with smaller probes reduces this overhead.


