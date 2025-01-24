use clap::Parser;
use s2n_quic_dc::{
    msg::addr::Addr,
    stream::{recv::router::Router, socket::Socket},
};
use std::{
    future::poll_fn,
    io::IoSlice,
    net::ToSocketAddrs,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{io::unix::AsyncFd, task::JoinSet};

#[derive(Parser, Debug)]
enum Args {
    Client(ClientArgs),
    Server(ServerArgs),
}

impl Args {
    async fn run(&self) {
        match self {
            Self::Client(args) => args.run().await,
            Self::Server(args) => args.run().await,
        }
    }
}

fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(1)
}

#[derive(Parser, Debug)]
struct ClientArgs {
    #[clap(long, default_value_t = 0)]
    port: u16,
    #[clap(long, default_value_t = default_concurrency())]
    concurrency: usize,
    #[clap(long, default_value_t = 1000)]
    payload: usize,
    server: Vec<String>,
}

impl ClientArgs {
    async fn run(&self) {
        let mut tasks = JoinSet::new();
        let sockets = send::pool(self.concurrency).unwrap();

        let servers: Arc<[_]> = self
            .server
            .iter()
            .flat_map(|host| host.to_socket_addrs().unwrap().next())
            .map(|addr| Addr::new(addr.into()))
            .collect();

        let count = Arc::new(AtomicU64::new(0));

        for socket in sockets {
            let socket = AsyncFd::new(socket).unwrap();
            let servers = servers.clone();
            let buffer = vec![0u8; self.payload];
            let count = count.clone();

            tasks.spawn(async move {
                let ecn = Default::default();
                loop {
                    let buffer = IoSlice::new(&buffer);
                    for addr in servers.iter() {
                        let _ = poll_fn(|cx| socket.poll_send(cx, addr, ecn, &[buffer])).await;
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }

        let payload = self.payload;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            let payload_bits = (payload * 8) as f64;
            loop {
                interval.tick().await;
                let count = count.swap(0, Ordering::Relaxed);
                let mut bps = count as f64 * payload_bits;
                let mut prefix = "";

                if bps > 1_000_000_000.0 {
                    prefix = "G";
                    bps /= 1_000_000_000.0;
                } else if bps > 1_000_000.0 {
                    prefix = "M";
                    bps /= 1_000_000.0;
                } else if bps > 1_000.0 {
                    prefix = "K";
                    bps /= 1_000.0;
                }

                println!("{count} pkts/s, {bps} {prefix}bps");
            }
        });

        while tasks.join_next().await.is_some() {}
    }
}

#[derive(Parser, Debug)]
struct ServerArgs {
    #[clap(long, default_value_t = 0)]
    port: u16,
    #[clap(long, default_value_t = default_concurrency())]
    concurrency: usize,
}

impl ServerArgs {
    async fn run(&self) {
        let router = Router::default();

        recv::pool(self.port, self.concurrency, router);

        todo!();
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    Args::parse().run().await;
}

mod recv {
    use s2n_quic_dc::{
        msg::recv::Message,
        socket::{Options, ReusePort},
        stream::recv::{router::Router, socket::worker_udp},
    };
    use std::{
        io,
        net::{Ipv4Addr, SocketAddr},
        thread,
    };

    pub fn pool(port: u16, concurrency: usize, router: Router<Message>) {
        Pool {
            backlog: 1024,
            concurrency,
            local_addr: (Ipv4Addr::UNSPECIFIED, port).into(),
            next_id: 0,
            router,
        }
        .start()
        .unwrap();
    }

    struct Pool {
        backlog: usize,
        concurrency: usize,
        local_addr: SocketAddr,
        next_id: usize,
        router: Router<Message>,
    }

    impl Pool {
        #[inline]
        fn start(&mut self) -> io::Result<()> {
            self.spawn_count(self.concurrency)?;
            debug_assert_ne!(self.local_addr.port(), 0, "a port should be selected");

            Ok(())
        }

        #[inline]
        fn spawn_count(&mut self, count: usize) -> io::Result<()> {
            for _ in 0..count {
                let socket = self.socket_opts().build_udp()?;
                self.spawn_udp(socket)?;
            }

            Ok(())
        }

        #[inline]
        fn socket_opts(&self) -> Options {
            let mut options = Options::new(self.local_addr);

            options.backlog = self.backlog;
            options.blocking = true;

            // if we have more than one thread then we'll need to use reuse port
            if self.concurrency > 1 {
                // if the application is wanting to bind to a random port then we need to set
                // reuse_port after
                if self.local_addr.port() == 0 {
                    options.reuse_port = ReusePort::AfterBind;
                } else {
                    options.reuse_port = ReusePort::BeforeBind;
                }
            }

            options
        }

        #[inline]
        fn spawn_udp(&mut self, socket: std::net::UdpSocket) -> io::Result<()> {
            // if this is the first socket being spawned then update the local address
            if self.local_addr.port() == 0 {
                self.local_addr = socket.local_addr()?;
            }

            let router = self.router.clone();
            thread::spawn(move || worker_udp(socket, router));

            Ok(())
        }

        fn id(&mut self) -> usize {
            let id = self.next_id;
            self.next_id += 1;
            id
        }
    }
}

mod send {
    use s2n_quic_dc::socket::Options;
    use std::{
        io,
        net::{Ipv4Addr, UdpSocket},
    };

    pub fn pool(mut concurrency: usize) -> io::Result<Vec<UdpSocket>> {
        concurrency = concurrency.max(1);

        let options = Options::new((Ipv4Addr::UNSPECIFIED, 0).into());
        let mut sockets = Vec::with_capacity(concurrency);

        for _ in 0..concurrency {
            let socket = options.build_udp()?;
            sockets.push(socket);
        }

        Ok(sockets)
    }
}
