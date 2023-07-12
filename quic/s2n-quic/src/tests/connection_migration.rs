// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn run_test<F>(mut on_rebind: F)
where
    F: FnMut(SocketAddr) -> SocketAddr + Send + 'static,
{
    let model = Model::default();
    let rtt = Duration::from_millis(10);
    let rebind_rate = rtt * 2;
    // we currently only support 4 migrations
    let rebind_count = 4;

    model.set_delay(rtt / 2);

    let expected_paths = Arc::new(Mutex::new(vec![]));
    let expected_paths_pub = expected_paths.clone();

    let on_socket = move |socket: io::Socket| {
        spawn(async move {
            let mut local_addr = socket.local_addr().unwrap();
            for _ in 0..rebind_count {
                local_addr = on_rebind(local_addr);
                delay(rebind_rate).await;
                if let Ok(mut paths) = expected_paths_pub.lock() {
                    paths.push(local_addr);
                }
                socket.rebind(local_addr);
            }
        });
    };

    let active_paths = recorder::ActivePathUpdated::new();
    let active_path_sub = active_paths.clone();

    test(model, move |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((events(), active_path_sub))?
            .start()?;

        let client_io = handle.builder().on_socket(on_socket).build()?;

        let client = Client::builder()
            .with_io(client_io)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(events())?
            .start()?;

        let addr = start_server(server)?;
        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();

            stream.send(Bytes::from_static(b"A")).await.unwrap();

            delay(rebind_rate / 2).await;

            for _ in 0..rebind_count {
                stream.send(Bytes::from_static(b"B")).await.unwrap();
                delay(rebind_rate).await;
            }

            stream.finish().unwrap();

            let chunk = stream
                .receive()
                .await
                .unwrap()
                .expect("a chunk should be available");
            assert_eq!(&chunk[..], &b"ABBBB"[..]);

            assert!(
                stream.receive().await.unwrap().is_none(),
                "stream should be finished"
            );
        });

        Ok(addr)
    })
    .unwrap();

    assert_eq!(
        &*active_paths.events().lock().unwrap(),
        &*expected_paths.lock().unwrap()
    );
}

/// Rebinds the IP of an address
fn rebind_ip(mut addr: SocketAddr) -> SocketAddr {
    let ip = match addr.ip() {
        std::net::IpAddr::V4(ip) => {
            let mut v = u32::from_be_bytes(ip.octets());
            v += 1;
            std::net::Ipv4Addr::from(v).into()
        }
        std::net::IpAddr::V6(ip) => {
            let mut v = u128::from_be_bytes(ip.octets());
            v += 1;
            std::net::Ipv6Addr::from(v).into()
        }
    };
    addr.set_ip(ip);
    addr
}

/// Rebinds the port of an address
fn rebind_port(mut addr: SocketAddr) -> SocketAddr {
    let port = addr.port() + 1;
    addr.set_port(port);
    addr
}

#[test]
fn ip_rebind_test() {
    run_test(rebind_ip);
}

#[test]
fn port_rebind_test() {
    run_test(rebind_port);
}

#[test]
fn ip_and_port_rebind_test() {
    run_test(|addr| rebind_ip(rebind_port(addr)));
}
