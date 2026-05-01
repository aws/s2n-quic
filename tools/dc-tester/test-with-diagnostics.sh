#!/bin/bash
# Run dc-tester with diagnostics to identify packet loss root cause

set -e

echo "=== Starting packet loss diagnostics ==="
echo "Date: $(date)"
echo

# Check current kernel settings
echo "=== Kernel UDP/Network Settings ==="
sysctl -a 2>/dev/null | grep -E "rmem|wmem|netdev_max_backlog|udp_mem" || true
echo

# Check network interface stats before test
echo "=== Network Interface Stats (BEFORE) ==="
netstat -su 2>/dev/null | grep -E "packet|error|buffer|drop" || true
echo

# Get interface stats
INTERFACE=$(ip route get 8.8.8.8 2>/dev/null | grep -oP 'dev \K\S+' || echo "eth0")
echo "Primary interface: $INTERFACE"
echo "RX/TX errors before:"
ip -s link show "$INTERFACE" 2>/dev/null | grep -A 2 "RX:\|TX:" || true
echo

echo "=== Starting test (check S2N_LOG output for ACK buffer warnings) ==="
echo "Look for: 'ACK buffer overflow' or 'ACK send blocked' in trace logs"
echo

# Note: User should run their actual test here with S2N_LOG=trace
echo "Run your dc-tester now with: S2N_LOG=warn cargo run ..."
echo "Press Ctrl+C when done"
echo

read -p "Press Enter when test is complete to see diagnostics..."

echo
echo "=== Network Interface Stats (AFTER) ==="
netstat -su 2>/dev/null | grep -E "packet|error|buffer|drop" || true
echo

echo "RX/TX errors after:"
ip -s link show "$INTERFACE" 2>/dev/null | grep -A 2 "RX:\|TX:" || true
echo

echo "=== Check for kernel drops ==="
dmesg | tail -50 | grep -iE "drop|loss|overflow|out of memory" || echo "No kernel drops found in recent dmesg"
echo

echo "=== jemalloc stats from logs ==="
echo "Check your test output for memory fragmentation and resident_mb growth"
