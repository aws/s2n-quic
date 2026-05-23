// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    process::Stdio,
    sync::Arc,
};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    signal,
    sync::{self, mpsc},
    task::JoinHandle,
};
use xshell::{Shell, cmd};

#[derive(Args)]
pub struct Local {
    /// Number of RPC server nodes to start
    #[arg(long, default_value = "1")]
    servers: usize,

    /// Number of RPC client nodes to start
    #[arg(long, default_value = "1")]
    clients: usize,

    /// Cargo build profile
    #[arg(default_value = "dev", long)]
    profile: String,

    /// Log level (env: S2N_LOG)
    #[arg(long, env = "S2N_LOG")]
    log_level: Option<String>,

    /// Path to configuration file specifying remote hosts
    #[arg(long, short)]
    config: Option<PathBuf>,

    /// Path to dc-tester config file
    #[arg(long)]
    dc_config: Option<PathBuf>,

    #[arg(long)]
    memory_dump: bool,

    /// Directory for diagnostic event traces (one JSON file per errored stream)
    #[arg(long, default_value = "/tmp/dc-traces")]
    trace_dir: PathBuf,

    /// Kind of test to run: dc-tester or wheel-demo
    #[arg(long, default_value = "dc-tester")]
    kind: String,

    /// Enable dial9 runtime telemetry (CPU flamegraphs + Tokio task traces) on all nodes
    #[arg(long)]
    dial9: bool,

    /// Workload names to run on client (defaults to first in config if omitted)
    #[arg(long, short)]
    workloads: Vec<String>,

    /// Directory for log output (default: logs/{workload}/{date})
    #[arg(long)]
    log_dir: Option<PathBuf>,

    /// Timeout in seconds (default: 30s when CLAUDECODE=1, unlimited otherwise)
    #[arg(long)]
    timeout: Option<u64>,

    #[arg(long)]
    print_metrics: Option<bool>,
}

impl Local {
    pub fn run(self, sh: &Shell) -> Result<()> {
        let claudecode = std::env::var("CLAUDECODE").is_ok();

        // Acquire an exclusive lock so concurrent runs block instead of clobbering
        let lock_path = PathBuf::from("/tmp/xtask-local.lock");
        let lock_file = std::fs::File::create(&lock_path).context("Failed to create lock file")?;
        use std::os::unix::io::AsRawFd;
        eprintln!("Acquiring lock...");
        let ret = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX) };
        if ret != 0 {
            anyhow::bail!(
                "Failed to acquire lock: {}",
                std::io::Error::last_os_error()
            );
        }
        // lock_file held for the duration of run — released on drop

        let mut nodes = Nodes::from_config(sh, &self.config)?;
        let (binary_dir, binary_name) = self.binary_info()?;

        eprintln!("Setting up nodes...");
        std::thread::scope(|s| {
            let mut handles = vec![];
            for node in nodes.iter_mut() {
                let sh = sh.clone();
                handles.push(s.spawn(move || node.setup(&sh)));
            }
            for h in handles {
                h.join().unwrap()?;
            }
            <Result<()>>::Ok(())
        })?;

        eprintln!("Building code...");
        std::thread::scope(|s| {
            let mut handles = vec![];
            for node in nodes.iter_mut() {
                let sh = sh.clone();
                let profile = &self.profile;
                let kind = &self.kind;
                handles.push(s.spawn(move || {
                    node.deploy(&sh)?;
                    node.build(&sh, profile, kind)?;
                    <Result<()>>::Ok(())
                }));
            }
            for h in handles {
                h.join().unwrap()?;
            }
            <Result<()>>::Ok(())
        })?;

        let mut base_port = ports();
        let mut colors = colors();
        let mut processes = Vec::new();
        let mut server_addresses = Vec::new();

        let dc_config = self
            .dc_config
            .unwrap_or_else(|| PathBuf::from("tools/dc-tester/etc/config.example.toml"));

        let mut base_env = HashMap::new();
        for name in ["RUST_BACKTRACE"] {
            if let Ok(val) = std::env::var(name) {
                base_env.insert(name.to_string(), val);
            }
        }

        let dial9_run_id = if self.dial9 {
            let run_id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let trace_dir = format!("/tmp/dial9-traces/{run_id}");
            base_env.insert("DIAL9_ENABLED".into(), "1".into());
            base_env.insert("DIAL9_TRACE_DIR".into(), trace_dir);
            base_env.insert("DIAL9_CPU_PROFILE_ENABLED".into(), "1".into());
            Some(run_id)
        } else {
            None
        };

        // Start servers
        for i in 0..self.servers {
            let node = &nodes[i % nodes.len()];
            let node_base = base_port.next().unwrap();
            let color = colors.next().unwrap();

            let server_port = node_base;
            let server_addr = SocketAddr::new(node.ip(), server_port);
            server_addresses.push(server_addr);

            let label = format!("server:{i}");

            eprintln!("  {} → {} ({})", label, server_addr, node.host());

            let mut env_vars = base_env.clone();
            if let Some(ref log) = self.log_level {
                env_vars.insert("S2N_LOG".to_string(), log.to_string());
            }

            if self.memory_dump {
                // Enable jemalloc heap profiling with dumps every 1GB and at exit
                // Note: tikv-jemallocator uses --with-jemalloc-prefix=_rjem_ so the env var is _RJEM_MALLOC_CONF
                env_vars.insert(
                    "_RJEM_MALLOC_CONF".to_string(),
                    "prof:true,lg_prof_interval:30,lg_prof_sample:21,prof_prefix:/tmp/jeprof.server"
                        .to_string(),
                );
            }

            let (binary, config_path) =
                node.resolve_paths(sh, &binary_dir.join(&binary_name), &dc_config);

            let args = if self.kind == "wheel-demo" {
                // Bind to wildcard address to accept on all interfaces
                let bind_addr = SocketAddr::new(
                    if server_addr.is_ipv6() {
                        "::".parse().unwrap()
                    } else {
                        "0.0.0.0".parse().unwrap()
                    },
                    server_addr.port(),
                );
                vec![
                    "server".to_string(),
                    "--address".to_string(),
                    bind_addr.to_string(),
                ]
            } else {
                let trace_dir = self.trace_dir.display().to_string();
                let bind_addr = SocketAddr::new(
                    if server_addr.is_ipv6() {
                        "::".parse().unwrap()
                    } else {
                        "0.0.0.0".parse().unwrap()
                    },
                    server_addr.port(),
                );
                vec![
                    "--trace-dir".to_string(),
                    trace_dir,
                    "server".to_string(),
                    "--config".to_string(),
                    config_path,
                    "--address".to_string(),
                    bind_addr.to_string(),
                ]
            };

            processes.push(ProcessConfig {
                target: node.clone(),
                label,
                binary,
                args,
                env_vars,
                color,
            });
        }

        // Start clients
        for i in 0..self.clients {
            // Offset by number of servers to distribute across different nodes
            let node = &nodes[(self.servers + i) % nodes.len()];
            let _node_base = base_port.next().unwrap();
            let color = colors.next().unwrap();

            let label = format!("client:{i}");

            eprintln!(
                "  {} ({}) connecting to {:?}",
                label,
                node.host(),
                server_addresses
            );

            let mut env_vars = base_env.clone();
            if let Some(ref log) = self.log_level {
                env_vars.insert("S2N_LOG".to_string(), log.to_string());
            }

            if self.memory_dump {
                // Enable jemalloc heap profiling with dumps every 1GB and at exit
                // Note: tikv-jemallocator uses --with-jemalloc-prefix=_rjem_ so the env var is _RJEM_MALLOC_CONF
                env_vars.insert(
                    "_RJEM_MALLOC_CONF".to_string(),
                    "prof:true,lg_prof_interval:30,lg_prof_sample:21,prof_prefix:/tmp/jeprof.client"
                        .to_string(),
                );
            }

            let (binary, config_path) =
                node.resolve_paths(sh, &binary_dir.join(&binary_name), &dc_config);

            let args = if self.kind == "wheel-demo" {
                vec![
                    "client".to_string(),
                    "--server".to_string(),
                    server_addresses[i % server_addresses.len()].to_string(),
                ]
            } else {
                let trace_dir = self.trace_dir.display().to_string();
                let mut args = vec![
                    "--trace-dir".to_string(),
                    trace_dir,
                    "client".to_string(),
                    "--config".to_string(),
                    config_path,
                ];
                for addr in &server_addresses {
                    args.push("--server-addr".to_string());
                    args.push(addr.to_string());
                }
                for w in &self.workloads {
                    args.push("--workloads".to_string());
                    args.push(w.clone());
                }
                args
            };

            processes.push(ProcessConfig {
                target: node.clone(),
                label,
                binary,
                args,
                env_vars,
                color,
            });
        }

        let workload_name = self
            .workloads
            .first()
            .map(|s| s.as_str())
            .unwrap_or("default");
        let log_dir = self.log_dir.unwrap_or_else(|| {
            let date = chrono_date();
            PathBuf::from("logs").join(workload_name).join(date)
        });

        let timeout = self
            .timeout
            .or_else(|| if claudecode { Some(30) } else { None });

        if let Some(t) = timeout {
            eprintln!("Timeout: {t}s");
        }
        eprintln!("Log dir: {}\n", log_dir.display());
        eprintln!("Starting processes...\n");

        let run_name = log_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let run_config = RunConfig {
            log_dir,
            timeout,
            run_name,
            workloads: self.workloads,
            ansi: !claudecode,
            print_metrics: self.print_metrics.unwrap_or_else(|| {
                // Reduce token usage
                !claudecode
            }),
        };
        let result = run_processes(sh.clone(), processes, run_config);

        if let Some(run_id) = dial9_run_id {
            let local_trace_dir = format!("/tmp/dial9-traces/{run_id}");
            std::fs::create_dir_all(&local_trace_dir).ok();

            for node in nodes.nodes.iter() {
                if let Node::Remote(remote) = node {
                    let src = format!(
                        "{}:/tmp/dial9-traces/{run_id}/",
                        remote.ssh_target()
                    );
                    let dest = format!("{}/{}/", local_trace_dir, remote.host);
                    std::fs::create_dir_all(&dest).ok();
                    let _ = cmd!(sh, "rsync -avz {src} {dest}").quiet().run();
                }
            }

            eprintln!("Dial9 traces: {local_trace_dir}");
            eprintln!("View with: dial9 serve {local_trace_dir}");
        }

        result
    }

    fn binary_info(&self) -> Result<(PathBuf, String)> {
        let profile = &self.profile;
        let dir = if profile == "dev" { "debug" } else { profile };

        let binary_name = match self.kind.as_str() {
            "wheel-demo" => "wheel-demo",
            _ => "dc-tester",
        };

        // Both binaries are part of the main workspace, so they go to the
        // workspace root's target/<profile>/<binary>
        Ok((PathBuf::from("target").join(dir), binary_name.to_string()))
    }
}

#[derive(Clone, Debug)]
enum Node {
    Local,
    Remote(RemoteNode),
}

impl Node {
    fn host(&self) -> String {
        match self {
            Self::Local => "localhost".into(),
            Self::Remote(node) => node.host.clone(),
        }
    }

    fn setup(&mut self, sh: &Shell) -> Result<()> {
        match self {
            Self::Local => Ok(()),
            Self::Remote(node) => node.setup(sh),
        }
    }

    fn deploy(&self, sh: &Shell) -> Result<()> {
        match self {
            Self::Local => Ok(()),
            Self::Remote(node) => node.deploy(sh),
        }
    }

    fn build(&self, sh: &Shell, profile: &str, kind: &str) -> Result<()> {
        match self {
            Self::Local => {
                let (path_prefix, binary_name) = match kind {
                    "wheel-demo" => ("tools/wheel-demo", "wheel-demo"),
                    _ => ("tools/dc-tester", "dc-tester"),
                };

                cmd!(
                    sh,
                    "cargo build --manifest-path {path_prefix}/Cargo.toml --profile {profile}"
                )
                .run()
                .with_context(|| format!("Failed to build {}", binary_name))?;

                // Set capabilities for thread priority control on Linux
                #[cfg(target_os = "linux")]
                {
                    let dir = if profile == "dev" { "debug" } else { profile };
                    let binary_path = PathBuf::from(path_prefix)
                        .join("target")
                        .join(dir)
                        .join(binary_name);
                    let binary = binary_path.display().to_string();

                    // Try common locations for setcap
                    let setcap_paths = ["/usr/sbin/setcap", "/sbin/setcap", "setcap"];
                    let mut success = false;
                    for setcap in setcap_paths {
                        if let Ok(_) = cmd!(sh, "sudo {setcap} cap_sys_nice=eip {binary}")
                            .quiet()
                            .run()
                        {
                            eprintln!("Set CAP_SYS_NICE capability on {} binary", binary_name);
                            success = true;
                            break;
                        }
                    }
                    if !success {
                        eprintln!(
                            "Warning: Failed to set capabilities. Thread priority scheduling will not work."
                        );
                    }
                }

                Ok(())
            }
            Self::Remote(node) => node.build(sh, profile, kind),
        }
    }

    fn ip(&self) -> IpAddr {
        match self {
            Self::Local => std::net::Ipv6Addr::LOCALHOST.into(),
            Self::Remote(node) => node.ip.unwrap(),
        }
    }

    /// Returns (binary_path, config_path) appropriate for local or remote execution
    fn resolve_paths(
        &self,
        sh: &Shell,
        local_binary: &std::path::Path,
        local_config: &std::path::Path,
    ) -> (PathBuf, String) {
        match self {
            Self::Local => (
                sh.current_dir().join(local_binary),
                sh.current_dir().join(local_config).display().to_string(),
            ),
            Self::Remote(remote) => {
                // On remote, binary and config are relative to remote_dir
                (
                    remote.dir.join(local_binary),
                    remote.dir.join(local_config).display().to_string(),
                )
            }
        }
    }
}

#[derive(Clone, Debug)]
struct RemoteNode {
    user: Option<String>,
    host: String,
    dir: PathBuf,
    ip: Option<IpAddr>,
}

impl RemoteNode {
    fn ssh_target(&self) -> String {
        if let Some(user) = &self.user {
            format!("{}@{}", user, self.host)
        } else {
            self.host.clone()
        }
    }

    fn setup(&mut self, sh: &Shell) -> Result<()> {
        let target = self.ssh_target();
        let dir = self.dir.as_path();

        let max_socket_buf = 200_000_000;
        let script = format!(
            r#"
# Create working directory
mkdir -p {dir}

# Configure socket buffer sizes
sudo sysctl -w net.core.wmem_max={max_socket_buf}
sudo sysctl -w net.core.rmem_max={max_socket_buf}
sudo sysctl -w net.core.netdev_budget=600

# Install required packages
command -v setcap >/dev/null 2>&1 || sudo yum install -y libcap >/dev/null 2>&1 || true
command -v tc >/dev/null 2>&1 || sudo yum install -y iproute-tc >/dev/null 2>&1 || true

# Configure qdisc to match production: mq root with fq_codel per TX queue
# This prevents packet drops from qdisc memory limits (32MB per queue vs 32MB total)
IFACE=$(ip route get 8.8.8.8 2>/dev/null | awk '{{ for(i=1;i<=NF;i++) if ($i == "dev") print $(i+1); }}' | head -1)
if [ -n "$IFACE" ]; then
    sudo tc qdisc del dev $IFACE root 2>/dev/null || true
    sudo tc qdisc add dev $IFACE root handle 1: mq 2>/dev/null
    for i in 1 2 3 4 5 6 7 8; do
        sudo tc qdisc add dev $IFACE parent 1:$i fq_codel memory_limit 256Mb 2>/dev/null || true
    done
fi

# Configure RT priority limits for the current user
grep -q '^[^#]*rtprio' /etc/security/limits.conf || echo "$USER - rtprio 99" | sudo tee -a /etc/security/limits.conf >/dev/null
grep -q '^[^#]*nice' /etc/security/limits.conf || echo "$USER - nice -20" | sudo tee -a /etc/security/limits.conf >/dev/null

# Enable RT scheduling in user cgroup (systemd default disables this)
echo 950000 | sudo tee /sys/fs/cgroup/cpu,cpuacct/user.slice/cpu.rt_runtime_us >/dev/null 2>&1 || true
"#,
            dir = dir.display(),
            max_socket_buf = max_socket_buf
        );

        cmd!(sh, "ssh {target} {script}")
            .quiet()
            .run()
            .context("Failed to set up remote node")?;

        // Resolve the remote_dir (expand ~ to actual home dir)
        let dir_str = dir.display().to_string();
        if dir_str.starts_with("~/") {
            let home = cmd!(sh, "ssh {target} echo $HOME")
                .quiet()
                .read()
                .context("Failed to resolve remote HOME")?;
            let home = home.trim();
            self.dir = PathBuf::from(format!("{}/{}", home, &dir_str[2..]));
        }

        // Resolve IP
        let output = cmd!(sh, "ssh {target} hostname -I")
            .quiet()
            .read()
            .context("Failed to resolve node IP address")?;

        let mut ips: Vec<IpAddr> = output
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .filter(|ip: &IpAddr| !(ip.is_loopback() || ip.is_unspecified()))
            .collect();

        // Prefer IPv6
        ips.sort_by(|a, b| match (a, b) {
            (IpAddr::V6(_), IpAddr::V4(_)) => std::cmp::Ordering::Greater,
            (IpAddr::V4(_), IpAddr::V6(_)) => std::cmp::Ordering::Less,
            _ => a.cmp(b),
        });

        let ip = *ips.first().unwrap();
        self.ip = Some(ip);
        eprintln!("  {} -> {}", target, ip);

        Ok(())
    }

    fn deploy(&self, sh: &Shell) -> Result<()> {
        let workspace_root = sh.current_dir();
        let node = self.ssh_target();
        let remote_dir = self.dir.as_path();

        let src = format!("{}/", workspace_root.display());
        let dest = format!("{}:{}/", node, remote_dir.display());

        let rsync_args = [
            "--exclude=target/*",
            "--exclude=logs/*",
            "--exclude=.git/",
            "--exclude=.claude",
            "-avz",
            "--delete",
            &src,
            &dest,
        ];

        cmd!(sh, "rsync {rsync_args...}")
            .quiet()
            .run()
            .context("Failed to rsync code to remote node")?;

        Ok(())
    }

    fn build(&self, sh: &Shell, profile: &str, kind: &str) -> Result<()> {
        let node = self.ssh_target();
        let remote_dir = self.dir.as_path();

        let (path_prefix, binary_name) = match kind {
            "wheel-demo" => ("tools/wheel-demo", "wheel-demo"),
            _ => ("tools/dc-tester", "dc-tester"),
        };

        let build_cmd = format!(
            "cd {} && cargo build --manifest-path {}/Cargo.toml --profile {profile}",
            remote_dir.display(),
            path_prefix
        );

        let shell_cmd = format!("bash --login -c {build_cmd:?}");

        cmd!(sh, "ssh {node} {shell_cmd}")
            .quiet()
            .run()
            .context("Failed to build on remote node")?;

        // Set capabilities on the remote binary
        let dir = if profile == "dev" { "debug" } else { profile };
        let binary_path = format!("{}/target/{}/{}", remote_dir.display(), dir, binary_name);
        let setcap_cmd = format!(
            "sudo /usr/sbin/setcap cap_sys_nice=eip {binary_path} 2>&1 || echo 'Failed to set capabilities'"
        );

        match cmd!(sh, "ssh {node} {setcap_cmd}").read() {
            Ok(output) => {
                if output.contains("Failed") {
                    eprintln!(
                        "Warning: Failed to set capabilities on remote node: {}",
                        output
                    );
                } else {
                    eprintln!("Set capabilities on remote binary");
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to set capabilities on remote node: {}", e);
            }
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct LocalConfig {
    #[serde(default)]
    host: Vec<HostConfig>,
    #[serde(default = "LocalConfig::default_remote_dir")]
    remote_dir: PathBuf,
}

impl LocalConfig {
    fn default_remote_dir() -> PathBuf {
        PathBuf::from("~/s2n-quic")
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct HostConfig {
    hostname: String,
    #[serde(default)]
    user: Option<String>,
}

struct Nodes {
    nodes: Vec<Node>,
}

impl Nodes {
    fn from_config(sh: &Shell, config_path: &Option<PathBuf>) -> Result<Self> {
        if let Some(path) = config_path {
            let content = sh
                .read_file(path)
                .with_context(|| format!("Failed to read config file: {}", path.display()))?;
            let cfg: LocalConfig = toml::from_str(&content)
                .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

            let mut nodes: Vec<Node> = cfg
                .host
                .iter()
                .map(|host| {
                    Node::Remote(RemoteNode {
                        user: host.user.clone(),
                        host: host.hostname.clone(),
                        dir: cfg.remote_dir.clone(),
                        ip: None,
                    })
                })
                .collect();

            if nodes.is_empty() {
                nodes.push(Node::Local);
            }

            Ok(Self { nodes })
        } else {
            Ok(Self {
                nodes: vec![Node::Local],
            })
        }
    }

    fn iter_mut(&mut self) -> std::slice::IterMut<'_, Node> {
        self.nodes.iter_mut()
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }
}

impl std::ops::Index<usize> for Nodes {
    type Output = Node;
    fn index(&self, index: usize) -> &Self::Output {
        &self.nodes[index]
    }
}

struct RunConfig {
    log_dir: PathBuf,
    timeout: Option<u64>,
    run_name: String,
    workloads: Vec<String>,
    ansi: bool,
    print_metrics: bool,
}

fn chrono_date() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let secs_per_day = 86400;
    let days = now / secs_per_day;
    // Simple date: YYYY-MM-DD_HHMMSS
    let time_of_day = now % secs_per_day;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    // Approximate date from epoch days
    let (year, month, day) = epoch_days_to_date(days);
    format!("{year:04}-{month:02}-{day:02}_{hours:02}{minutes:02}{seconds:02}")
}

fn epoch_days_to_date(days: u64) -> (u64, u64, u64) {
    // Civil days algorithm from Howard Hinnant
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

struct ProcessConfig {
    target: Node,
    label: String,
    binary: PathBuf,
    args: Vec<String>,
    env_vars: HashMap<String, String>,
    color: Color,
}

fn colors() -> impl Iterator<Item = Color> {
    [
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
    ]
    .into_iter()
    .cycle()
}

fn ports() -> impl Iterator<Item = u16> {
    (0..).map(|v| 4433 + v * 100)
}

#[tokio::main]
async fn run_processes(
    _sh: Shell,
    configs: Vec<ProcessConfig>,
    run_config: RunConfig,
) -> Result<()> {
    use std::io::Write;

    let max_label_len = configs.iter().map(|c| c.label.len()).max().unwrap_or(0);

    // Set up log directory and files
    std::fs::create_dir_all(&run_config.log_dir)
        .with_context(|| format!("Failed to create log dir: {}", run_config.log_dir.display()))?;
    let log_path = run_config.log_dir.join("output.log");
    let metrics_path = run_config.log_dir.join("metrics.jsonl");
    let mut log_file = std::fs::File::create(&log_path)
        .with_context(|| format!("Failed to create log file: {}", log_path.display()))?;
    let mut metrics_file = std::fs::File::create(&metrics_path)
        .with_context(|| format!("Failed to create metrics file: {}", metrics_path.display()))?;

    let mut children = Vec::new();
    let (tx, mut rx) = mpsc::channel(512);

    for config in &configs {
        let child = spawn_process(config, tx.clone()).await?;
        children.push((config.label.clone(), child));
    }

    drop(tx);

    let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
        .context("Failed to set up SIGINT handler")?;
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
        .context("Failed to set up SIGTERM handler")?;

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);

    let (shutdown, mut exit_task) = monitor_processes(children);

    let timeout_at = run_config
        .timeout
        .map(|secs| tokio::time::Instant::now() + tokio::time::Duration::from_secs(secs));
    let timeout_fut = async {
        match timeout_at {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(timeout_fut);

    loop {
        tokio::select! {
            Some((color, label, line)) = rx.recv() => {
                // Check for metrics prefix — line may be "...INFO [METRICS] raw"
                if let Some((prefix, raw)) = line.split_once("[METRICS]") {
                    let raw = raw.trim();
                    if raw.is_empty() {
                        // Heartbeat — skip
                    } else {
                        let parsed = s2n_quic_dc_metrics::format::ParsedMetricsLine::parse(raw);

                        if run_config.print_metrics {
                            // Pretty-print to console, preserving the timestamp prefix
                            let pretty = parsed.format_pretty();
                            print_line(&mut stdout, color, &label, format_args!("{} {}", prefix.trim_end(), pretty), max_label_len)?;
                        }

                        // Write structured JSONL — one row per metric
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs_f64();
                        for mut row in parsed.to_json_rows() {
                            if let Some(obj) = row.as_object_mut() {
                                obj.insert("process".into(), serde_json::Value::String(label.to_string()));
                                obj.insert("ts".into(), serde_json::json!(ts));
                                obj.insert("run".into(), serde_json::Value::String(run_config.run_name.clone()));
                                obj.insert("workloads".into(), serde_json::json!(run_config.workloads));
                            }
                            let _ = writeln!(metrics_file, "{}", row);
                        }
                    }
                } else {
                    // Non-metrics line: write to log and console
                    let stripped = strip_ansi_escapes::strip(&line);
                    let stripped = String::from_utf8_lossy(&stripped);
                    let _ = writeln!(log_file, "[{label}] {stripped}");
                    let line = if run_config.ansi {
                        &*line
                    } else {
                        &*stripped
                    };
                    print_line(&mut stdout, color, &label, line, max_label_len)?;
                }
            }
            _ = sigint.recv() => {
                eprintln!("\nReceived Ctrl+C, shutting down...\n");
                break;
            }
            _ = sigterm.recv() => {
                eprintln!("\nReceived SIGTERM, shutting down...\n");
                break;
            }
            _ = &mut exit_task => {
                eprintln!("\nChild processes exited, shutting down...\n");
                break;
            }
            _ = &mut timeout_fut => {
                eprintln!("\nTimeout reached, shutting down...\n");
                break;
            }
        }
    }

    drop(shutdown);

    if !exit_task.is_finished() {
        let _ = exit_task.await;
    }

    eprintln!("Log: {}", log_path.display());
    eprintln!("Metrics: {}", metrics_path.display());

    Ok(())
}

async fn spawn_process(
    config: &ProcessConfig,
    tx: mpsc::Sender<(Color, Arc<str>, String)>,
) -> Result<Child> {
    let mut child = match &config.target {
        Node::Local => {
            let mut cmd = Command::new(&config.binary);
            cmd.args(&config.args)
                .envs(&config.env_vars)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);

            cmd.spawn()
                .with_context(|| format!("Failed to spawn process: {:?}", config.binary))?
        }
        Node::Remote(remote) => {
            let mut cmd = Command::new("ssh");
            cmd.arg("-tt").arg(remote.ssh_target());

            let mut remote_cmd = String::new();
            remote_cmd.push_str("stty -opost; ");
            for (key, value) in &config.env_vars {
                remote_cmd.push_str(&format!("export {}='{}'; ", key, value));
            }
            remote_cmd.push_str(&format!("cd {}; ", remote.dir.display()));

            remote_cmd.push_str(&format!("{}", config.binary.display()));
            for arg in &config.args {
                remote_cmd.push_str(&format!(" '{}'", arg));
            }

            cmd.arg(remote_cmd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);

            cmd.spawn()
                .with_context(|| format!("Failed to spawn remote process on {}", remote.host))?
        }
    };

    let stdout = child.stdout.take().context("Failed to capture stdout")?;

    let label: Arc<str> = config.label.clone().into();
    let color = config.color;

    let label_clone = label.clone();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_clone.send((color, label_clone.clone(), line)).await;
        }
    });

    if let Some(stderr) = child.stderr.take() {
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx_clone.send((color, label.clone(), line)).await;
            }
        });
    }

    Ok(child)
}

fn print_line(
    stdout: &mut StandardStream,
    color: Color,
    label: &str,
    line: impl core::fmt::Display,
    max_label_len: usize,
) -> Result<()> {
    use std::io::Write;

    stdout.set_color(ColorSpec::new().set_fg(Some(color)).set_bold(true))?;
    write!(stdout, "[{:width$}]", label, width = max_label_len)?;
    stdout.reset()?;
    writeln!(stdout, " {line}")?;

    Ok(())
}

fn monitor_processes(
    mut children: Vec<(String, Child)>,
) -> (sync::oneshot::Sender<()>, JoinHandle<()>) {
    let (shutdown_signal, mut on_shutdown) = sync::oneshot::channel::<()>();

    let task = tokio::spawn(async move {
        loop {
            let mut any_exited = false;
            for (label, child) in &mut children {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        eprintln!("\nProcess {} exited with status: {}", label, status);
                        any_exited = true;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("\nError checking process {}: {}", label, e);
                        any_exited = true;
                    }
                }
            }
            if any_exited {
                break;
            }
            tokio::select! {
                _ = &mut on_shutdown => break,
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
            }
        }

        cleanup_processes(&mut children).await;
    });

    (shutdown_signal, task)
}

async fn cleanup_processes(children: &mut Vec<(String, Child)>) {
    eprintln!("\nCleaning up processes...\n");

    for (label, child) in children.iter_mut() {
        if let Err(e) = child.start_kill() {
            eprintln!("Failed to kill process {}: {}", label, e);
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    for (label, child) in children.iter_mut() {
        match child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                eprintln!("Force killing process {}", label);
                let _ = child.kill().await;
            }
            Err(_) => {
                let _ = child.kill().await;
            }
        }
    }
}
