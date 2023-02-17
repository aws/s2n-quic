# Jumbo Frame Support
The standard ethernet Maximum Transmission Unit - MTU - is 1500 bytes. Jumbo frames
are ethernet frames that support more than 1500 bytes. Jumbo frames will often support
approximately 9000 bytes, although this is an implementation specific detail and you
should check to see what your network supports.

## why use jumbo frames
There are a number of overheads that occur per-packet. For example, udp packet
headers are included on each packet. If you can use larger datagrams, then the
communication is more efficient because the useable payload is larger relative
to the overhead.

There are also CPU utilization savings for jumbo frames as well

## how to use jumbo frames
You can only use jumbo frames if there are supported on the network that you are
communicating over. As an example, AWS supports jumbo frames in the specific
circumstances listed [here](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/network_mtu.html)

For this example, we can use the `tracepath` utility to make sure jumbo frames
are supported on our local machine.

```console
[ec2-user@ip-1-1-1-1 jumbo-frame]$ tracepath localhost
 1:  localhost                                             0.035ms reached
     Resume: pmtu 65535 hops 1 back 1
```
The tracepath tool says something about `pmtu 65535`, which means that the
Path Maximum Transmission Unit is 65,535 bytes. So 9,001 bytes should be no
problem!

Then we just need to configure the io provider that we pass into our quic
endpoint and we are good to go. The full code is in the folder, but the most
relevant snippet is reproduced below.
```rust
    let address: SocketAddr = "127.0.0.1:4433".parse()?;
    let io = s2n_quic::provider::io::Default::builder()
        .with_receive_address(address)?
        // Specify that the endpoint should try to use frames up to 9001 bytes.
        // This is the maximum mtu that most ec2 instances will support.
        // https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/network_mtu.html
        .with_max_mtu(9001)?
        // It's wise to benchmark for your individual usecase, but for the high
        // throughput scenarios that jumbo frames sometimes enable, it is wise
        // to set larger buffers on the sockets.
        .with_recv_buffer_size(12_000_000)?
        .with_send_buffer_size(12_000_000)?
        .build()?;
```

We can run the demo by spinning up two terminals. In the first terminal, start the server
```console
[ec2-user@ip-172-31-28-149 jumbo-frame]$ pwd
/home/ec2-user/workspace/s2n-quic/examples/jumbo-frame
[ec2-user@ip-172-31-28-149 jumbo-frame]$ cargo run --bin jumbo_server
   Compiling rustls-provider v0.1.0 (/home/ec2-user/workspace/s2n-quic/examples/jumbo-frame)
    Finished dev [unoptimized + debuginfo] target(s) in 3.23s
     Running `target/debug/jumbo_server`
Listening for a connection

```

In the other terminal, go ahead and start the client
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

In the terminal we can see that larger mtu's are successfully negotiated.

note that both side must enable jumbo frames for either side to use them. If a client tried with `max_mtu=9001` tries to probe a server with `max_mtu=1500`, then the server will simply drop the probes larger than 1500, and the client will continue to use the 1472 byte mtu that it initially negotiated.

## ensure jumbo frames are being used
The s2n-quic events system has a specific event for an MTU update. This is the event that the subscriber defined in `lib.rs` is using the print out the
```
MtuUpdated { path_id: 0, mtu: 8943, cause: ProbeAcknowledged }
```
information that you see in the console. You can use this event to verify that jumbo frames are being used. Alternatey, you could use a tool like `tcpdump` along with a packet analysis tool like `wireshark` to confirm that jumbo packets are being sent.

## a note on probing strategy
Quic implements Datagram Packetization Layer Path Maximum Transmission Unit Discovery, or DPLPMTUD for short. This is decribed in [RFC8899](https://www.rfc-editor.org/info/rfc8899). The basic idea is that to determine if an `X` byte MTU is supported we can send a packet of `X` bytes.
- if it is acked -> supported
- if it is lost  -> not supported

In the console output, you might have noticed that we don't immediately probe for the 9001 byte mtu, and indeed it takes us several probes to get close to it. What gives?

The MTU updates happen in this process
1. 1200 byte mtu - the handshake frames are padded to 1200 bytes. So if we successfuly complete a handshake then we know that the connection supports a minimum of a 1200 byte MTU. From the quic RFC, this is the minimum mtu that quic can operate on.
2. 1472 byte mtu - the quic endpoint will send the first probe at `min(1500 - overhead, max_mtu - overhead)`. In this case the 1472 byte probe represents `1500 - UDP_HEADER - IPV4_HEADER`. We start with this value because it is the most likely.
3. binary search towards `max_mtu` until we're within some reasonable threshold of `max_mtu`.

We don't immediately start probing at large values because of scenarios where
- a large (9000ish) max mtu is specified at the endpoint
- the network only supports a small mtu (1500ish)
- the connection exchanges a small amount of data

If we only wanted to send 9000 bytes of application data, we would be sending
- 9000 bytes of application data
- 9000 bytes of probe
So the overhead of probing would be 50%. Starting with smaller probes reduces this overhead.


