// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::path::secret::map::Entry;
use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6},
    sync::Arc,
    time::SystemTime,
};

/// Serializes `peers` (writing every entry), reads them back, and returns the decoded entries
/// alongside the recorded `started_at` timestamp.
fn roundtrip(peers: &[SocketAddr]) -> (SystemTime, Vec<SocketAddr>) {
    let entries: Vec<Arc<Entry>> = peers.iter().map(|peer| Entry::fake(*peer, None)).collect();
    // Epoch is irrelevant without a recency filter configured.
    roundtrip_with(&entries, Epoch(0), |s| s)
}

/// Serializes `entries` through a [`Serializer`] configured by `configure` (at `current_epoch`),
/// reads them back, and returns the decoded peers alongside the recorded `started_at` timestamp.
fn roundtrip_with(
    entries: &[Arc<Entry>],
    current_epoch: Epoch,
    configure: impl FnOnce(SerializerBuilder) -> SerializerBuilder,
) -> (SystemTime, Vec<SocketAddr>) {
    let weak: Vec<Weak<Entry>> = entries.iter().map(Arc::downgrade).collect();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets");

    let serializer = configure(Serializer::builder(&path)).build().unwrap();
    serializer.serialize(&weak, current_epoch).unwrap();

    let entries = deserialize(serializer.path()).unwrap();
    let started_at = entries.started_at;
    let decoded = entries.map(|e| e.unwrap().peer).collect();

    (started_at, decoded)
}

#[test]
fn roundtrip_ipv4() {
    let peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 4433));
    let (_, decoded) = roundtrip(&[peer]);
    assert_eq!(decoded, vec![peer]);
}

#[test]
fn roundtrip_ipv6_minimal() {
    // flowinfo and scope_id are both zero, exercising the compact (tag 1) encoding.
    let peer = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 4433, 0, 0));
    let (_, decoded) = roundtrip(&[peer]);
    assert_eq!(decoded, vec![peer]);
}

#[test]
fn roundtrip_ipv6_full() {
    // Non-zero flowinfo/scope_id force the full (tag 2) encoding.
    let peer = SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
        4433,
        7,
        42,
    ));
    let (_, decoded) = roundtrip(&[peer]);
    assert_eq!(decoded, vec![peer]);
}

#[test]
fn roundtrip_multiple() {
    let peers = vec![
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1)),
        SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 2, 0, 0)),
        SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 3, 1, 2)),
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 4), 4)),
    ];
    let (_, decoded) = roundtrip(&peers);
    assert_eq!(decoded, peers);
}

#[test]
fn roundtrip_empty() {
    let (_, decoded) = roundtrip(&[]);
    assert!(decoded.is_empty());
}

#[test]
fn started_at_is_recent() {
    let before = SystemTime::now();
    let peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4433));
    let (started_at, _) = roundtrip(&[peer]);
    let after = SystemTime::now();

    // The timestamp is stored at one-second granularity, so allow the truncation slack.
    let one_sec = Duration::from_secs(1);
    assert!(started_at + one_sec >= before);
    assert!(started_at <= after + one_sec);
}

/// One epoch's worth of wall-clock time, used to express idle windows in the tests below. Epochs
/// advance once per cleaner cycle.
const EPOCH: Duration = Duration::from_secs(60);

#[test]
fn max_idle_filters_stale_entries() {
    let recent_peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1));
    let stale_peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 2));

    let recent = Entry::fake(recent_peer, None);
    let stale = Entry::fake(stale_peer, None);

    // Mark the two entries as accessed in different epochs.
    recent.set_accessed_addr(Epoch(10));
    stale.set_accessed_addr(Epoch(4));

    // At epoch 12 with a 5-epoch idle window, the cutoff is epoch 7: only `recent` (epoch 10)
    // survives, `stale` (epoch 4) is dropped.
    let (_, decoded) = roundtrip_with(&[recent, stale], Epoch(12), |s| s.with_max_idle(5 * EPOCH));
    assert_eq!(decoded, vec![recent_peer]);
}

#[test]
fn max_idle_boundary_is_inclusive() {
    let peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1));
    let entry = Entry::fake(peer, None);
    entry.set_accessed_addr(Epoch(7));

    // At epoch 10 with a 3-epoch idle window the cutoff is exactly epoch 7, and an entry accessed
    // at the cutoff is kept.
    let (_, decoded) = roundtrip_with(&[entry], Epoch(10), |s| s.with_max_idle(3 * EPOCH));
    assert_eq!(decoded, vec![peer]);
}

#[test]
fn max_idle_drops_never_accessed_entries() {
    let peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1));
    // A fake entry has never been accessed, so it sits at epoch 0 and is filtered out once the
    // cutoff climbs above 0.
    let entry = Entry::fake(peer, None);

    let (_, decoded) = roundtrip_with(&[entry], Epoch(5), |s| s.with_max_idle(EPOCH));
    assert!(decoded.is_empty());
}

#[test]
fn idle_window_wider_than_epoch_keeps_everything() {
    let peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1));
    let entry = Entry::fake(peer, None);
    entry.set_accessed_addr(Epoch(2));

    // When the idle window exceeds the current epoch the cutoff saturates at 0, so even
    // long-idle entries are retained.
    let (_, decoded) = roundtrip_with(&[entry], Epoch(3), |s| s.with_max_idle(100 * EPOCH));
    assert_eq!(decoded, vec![peer]);
}

#[test]
fn with_max_idle_rounds_to_nearest_epoch() {
    // Sub-cycle durations round to the nearest epoch, with a floor of one epoch for any non-zero
    // duration. Here ~1.5 epochs rounds up to 2: at epoch 5 the cutoff is epoch 3.
    let kept_peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1));
    let dropped_peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 2));

    let kept = Entry::fake(kept_peer, None);
    let dropped = Entry::fake(dropped_peer, None);
    kept.set_accessed_addr(Epoch(3));
    dropped.set_accessed_addr(Epoch(2));

    let (_, decoded) = roundtrip_with(&[kept, dropped], Epoch(5), |s| {
        s.with_max_idle(EPOCH + EPOCH / 2)
    });
    assert_eq!(decoded, vec![kept_peer]);
}

#[test]
fn max_size_stops_adding_entries() {
    // Each IPv4 entry encodes to 7 bytes (1 tag + 4 IP + 2 port). With a cap just above the
    // header/version/timestamp prefix, only a couple of entries fit before the writer trips the
    // limit and the rest are dropped.
    let peers: Vec<SocketAddr> = (0..100)
        .map(|i| SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, i as u8), i)))
        .collect();
    let entries: Vec<Arc<Entry>> = peers.iter().map(|peer| Entry::fake(*peer, None)).collect();
    let weak: Vec<Weak<Entry>> = entries.iter().map(Arc::downgrade).collect();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets");
    let serializer = Serializer::builder(&path).build().unwrap();

    // Prefix is HEADER + VERSION + 8-byte timestamp; allow ~3 more entries' worth of room.
    let prefix = (HEADER.len() + VERSION.len() + 8) as u64;
    serializer
        .serialize_with_max_size(&weak, Epoch(0), prefix + 20)
        .unwrap();

    let decoded: Vec<SocketAddr> = deserialize(serializer.path())
        .unwrap()
        .map(|e| e.unwrap().peer)
        .collect();

    // We stop only after exceeding the cap, so we get a few entries but far fewer than 100, and
    // they are a prefix of the input in iteration order.
    assert!(!decoded.is_empty());
    assert!(decoded.len() < peers.len());
    assert_eq!(decoded, peers[..decoded.len()]);
}

#[test]
fn build_rejects_missing_directory() {
    let dir = tempfile::tempdir().unwrap();
    // Parent directory does not exist.
    let path = dir.path().join("missing").join("secrets");

    let err = match Serializer::builder(&path).build() {
        Ok(_) => panic!("expected build to reject a missing destination directory"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn build_accepts_existing_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets");
    assert!(Serializer::builder(&path).build().is_ok());
}

#[test]
fn rejects_bad_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets");
    std::fs::write(&path, b"not the right header at all").unwrap();

    let err = match deserialize(&path) {
        Ok(_) => panic!("expected deserialize to reject a bad header"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn rejects_out_of_range_timestamp() {
    // A valid header and version followed by a `started_at` of u64::MAX seconds, which would
    // overflow `SystemTime` addition. It must surface as an error rather than panicking.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets");

    let mut bytes = Vec::new();
    bytes.extend_from_slice(HEADER.as_bytes());
    bytes.extend_from_slice(VERSION);
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();

    let err = match deserialize(&path) {
        Ok(_) => panic!("expected deserialize to reject an out-of-range timestamp"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}
