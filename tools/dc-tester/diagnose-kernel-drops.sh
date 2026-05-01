#!/bin/bash
# Script to diagnose kernel-level packet drops on EC2 instances
# Run this on both server and client EC2 instances while dc-tester is running

set -e

# Ensure sbin directories are in PATH for ip, tc, sysctl
export PATH="/usr/local/sbin:/usr/sbin:/sbin:$PATH"

INTERVAL=${1:-5}
IFACE=$(ip route get 8.8.8.8 | awk '{ for(i=1;i<=NF;i++) if ($i == "dev") print $(i+1); }' | head -1)

echo "=== Network Diagnostics for Packet Loss Investigation ==="
echo "Interface: $IFACE"
echo "Interval: ${INTERVAL}s"
echo "Started at: $(date)"
echo ""

# Create baseline snapshots
snapshot_udp_stats() {
    netstat -su 2>/dev/null | grep -E "packet receive errors|packets sent|RcvbufErrors|SndbufErrors|InErrors|OutErrors" || true
}

snapshot_iface_stats() {
    ip -s link show "$IFACE" | tail -4
}

snapshot_qdisc_stats() {
    tc -s qdisc show dev "$IFACE" 2>/dev/null || true
}

snapshot_softnet_stat() {
    cat /proc/net/softnet_stat
}

echo "=== Initial UDP Socket Stats ==="
BASELINE_UDP=$(snapshot_udp_stats)
echo "$BASELINE_UDP"
echo ""

echo "=== Initial Interface Stats ==="
BASELINE_IFACE=$(snapshot_iface_stats)
echo "$BASELINE_IFACE"
echo ""

echo "=== Initial Qdisc Stats ==="
BASELINE_QDISC=$(snapshot_qdisc_stats)
echo "$BASELINE_QDISC"
echo ""

echo "=== Socket Buffer Configuration ==="
echo "UDP write buffer min: $(sysctl -n net.ipv4.udp_wmem_min)"
echo "Core write buffer max: $(sysctl -n net.core.wmem_max)"
echo "Core write buffer default: $(sysctl -n net.core.wmem_default)"
echo "UDP read buffer min: $(sysctl -n net.ipv4.udp_rmem_min)"
echo "Core read buffer max: $(sysctl -n net.core.rmem_max)"
echo "Core read buffer default: $(sysctl -n net.core.rmem_default)"
echo "UDP mem pages: $(sysctl -n net.ipv4.udp_mem)"
echo ""

echo "=== TX Queue Length ==="
ip link show "$IFACE" | grep -o 'qlen [0-9]*'
echo ""

echo "=== TX Ring Size ==="
ethtool -g "$IFACE" 2>/dev/null || echo "ethtool not available or no permissions"
echo ""

echo "=== Qdisc Configuration ==="
tc qdisc show dev "$IFACE"
echo ""

echo "=== Monitoring for ${INTERVAL}s... ==="
echo ""
sleep "$INTERVAL"

echo "=== After ${INTERVAL}s: UDP Socket Stats Delta ==="
AFTER_UDP=$(snapshot_udp_stats)
echo "BEFORE:"
echo "$BASELINE_UDP"
echo ""
echo "AFTER:"
echo "$AFTER_UDP"
echo ""

echo "=== After ${INTERVAL}s: Interface Stats Delta ==="
AFTER_IFACE=$(snapshot_iface_stats)
echo "BEFORE:"
echo "$BASELINE_IFACE"
echo ""
echo "AFTER:"
echo "$AFTER_IFACE"
echo ""

echo "=== After ${INTERVAL}s: Qdisc Stats Delta ==="
AFTER_QDISC=$(snapshot_qdisc_stats)
echo "BEFORE:"
echo "$BASELINE_QDISC"
echo ""
echo "AFTER:"
echo "$AFTER_QDISC"
echo ""

echo "=== Softnet Stats (CPU-level drops) ==="
echo "Format: packets_processed dropped squeezed collision received_rps flow_limit_count"
cat /proc/net/softnet_stat
echo ""

echo "=== Socket Memory Usage ==="
cat /proc/net/sockstat | grep UDP
echo ""

echo "=== Active dc-tester Sockets ==="
ss -unp | grep dc-tester | head -20
echo ""

echo "=== Socket Send/Recv Queue Sizes ==="
ss -unm | grep -A1 dc-tester | head -40
echo ""

echo "Completed at: $(date)"
