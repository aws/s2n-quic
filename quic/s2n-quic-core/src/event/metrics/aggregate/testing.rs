// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::event::snapshot::Location;
use alloc::{collections::BTreeMap, sync::Arc};
use std::sync::Mutex;

use super::Units;

#[derive(Clone)]
pub struct Registry(Arc<Inner>);

impl Registry {
    #[track_caller]
    pub fn snapshot() -> Self {
        Self(Arc::new(Inner::snapshot()))
    }

    #[track_caller]
    pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
        Self(Arc::new(Inner::named_snapshot(name)))
    }

    pub fn no_snapshot() -> Self {
        Self(Arc::new(Inner::no_snapshot()))
    }

    pub fn subscriber<N: core::fmt::Display>(&self, name: N) -> super::Subscriber<Subscriber> {
        let name = name.to_string();
        let registry = self.clone();
        let log = registry
            .0
            .events
            .lock()
            .unwrap()
            .entry(name)
            .or_default()
            .clone();
        super::Subscriber::new(Subscriber { registry, log })
    }
}

type Log = Arc<Mutex<Vec<String>>>;

#[derive(Clone)]
pub struct Subscriber {
    // hold on to the registry so it stays open
    #[allow(dead_code)]
    registry: Registry,
    log: Log,
}

impl Subscriber {
    fn push<T: core::fmt::Display>(&self, line: T) {
        let line = line.to_string();
        if let Ok(mut events) = self.log.lock() {
            events.push(line);
        }
    }
}

struct Inner {
    events: Mutex<BTreeMap<String, Log>>,
    location: Option<Location>,
}

impl Inner {
    #[track_caller]
    pub fn snapshot() -> Self {
        let mut v = Self::no_snapshot();
        v.location = Location::from_thread_name();
        v
    }

    #[track_caller]
    pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
        let mut sub = Self::no_snapshot();
        sub.location = Some(Location::new(name));
        sub
    }

    pub fn no_snapshot() -> Self {
        Self {
            events: Default::default(),
            location: None,
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let Some(location) = self.location.as_ref() else {
            return;
        };

        if std::thread::panicking() {
            return;
        }

        let events = if let Ok(mut events) = self.events.lock() {
            core::mem::take(&mut *events)
        } else {
            Default::default()
        };

        let mut log = vec![];
        for (name, lines) in events.iter() {
            log.push(format!("=== {name} ==="));
            if let Ok(mut lines) = lines.lock() {
                log.extend(core::mem::take(&mut *lines));
            } else {
                log.push(" ** poisoned **".to_string());
            }
        }

        location.snapshot_log(&log);
    }
}

impl super::Registry for Subscriber {
    type Counter = Recorder;
    type BoolCounter = Recorder;
    type NominalCounter = Recorder;
    type Measure = Recorder;
    type Gauge = Recorder;
    type Timer = Recorder;
    type NominalTimer = Recorder;

    fn register_counter(&self, info: &'static super::Info) -> Self::Counter {
        Self::Counter::new(self, info, "count")
    }

    fn register_bool_counter(&self, info: &'static super::Info) -> Self::BoolCounter {
        Self::BoolCounter::new(self, info, "count")
    }

    fn register_nominal_counter(
        &self,
        info: &'static super::Info,
        variant: &'static super::info::Variant,
    ) -> Self::NominalCounter {
        Self::NominalCounter::new_nominal(self, info, variant, "count")
    }

    fn register_measure(&self, info: &'static super::Info) -> Self::Measure {
        Self::Measure::new(self, info, "measure")
    }

    fn register_gauge(&self, info: &'static super::Info) -> Self::Gauge {
        Self::Gauge::new(self, info, "gauge")
    }

    fn register_timer(&self, info: &'static super::Info) -> Self::Timer {
        Self::Timer::new(self, info, "timer")
    }

    fn register_nominal_timer(
        &self,
        info: &'static super::Info,
        variant: &'static super::info::Variant,
    ) -> Self::NominalTimer {
        Self::NominalTimer::new_nominal(self, info, variant, "timer")
    }
}

pub struct Recorder(Subscriber, &'static str);

impl Recorder {
    fn new(registry: &Subscriber, _info: &'static super::Info, name: &'static str) -> Self {
        Self(registry.clone(), name)
    }

    fn new_nominal(
        registry: &Subscriber,
        _info: &'static super::Info,
        _variant: &'static super::info::Variant,
        name: &'static str,
    ) -> Self {
        Self(registry.clone(), name)
    }
}

impl super::Recorder for Recorder {
    fn record<T: super::Metric>(&self, info: &'static super::Info, value: T) {
        let prefix = self.1;
        let name = info.name;
        let units = match info.units {
            Units::Bytes => "b",
            _ => "",
        };

        // redact certain metrics for the snapshot
        match (prefix, name.as_ref()) {
            ("count", "datagram_received.bytes.total")
            | ("count", "datagram_sent.bytes.total")
            | ("count", "packet_sent.bytes.total")
            | ("measure", "recovery_metrics.bytes_in_flight")
            | ("measure", "datagram_sent.bytes")
            | ("measure", "datagram_received.bytes")
            | ("measure", "packet_sent.bytes") => {
                return self
                    .0
                    .push(format_args!("{prefix}#{name}=[REDACTED]{units}"));
            }
            _ => {}
        }

        if value.is_duration() {
            self.0
                .push(format_args!("{prefix}#{name}={:?}", value.as_duration()))
        } else if value.is_f32() {
            self.0
                .push(format_args!("{prefix}#{name}={}{units}", value.as_f32()))
        } else if value.is_f64() {
            self.0
                .push(format_args!("{prefix}#{name}={}{units}", value.as_f64()))
        } else {
            self.0
                .push(format_args!("{prefix}#{name}={}{units}", value.as_u64()))
        }
    }
}

impl super::NominalRecorder for Recorder {
    fn record<T: super::Metric>(
        &self,
        info: &'static super::Info,
        variant: &'static super::info::Variant,
        value: T,
    ) {
        let prefix = self.1;
        let name = info.name;
        let variant = variant.name;
        let units = match info.units {
            Units::Bytes => "b",
            _ => "",
        };
        if value.is_duration() {
            self.0.push(format_args!(
                "{prefix}#{name}|{variant}={:?}",
                value.as_duration()
            ))
        } else if value.is_f32() {
            self.0.push(format_args!(
                "{prefix}#{name}|{variant}={}{units}",
                value.as_f32()
            ))
        } else if value.is_f64() {
            self.0.push(format_args!(
                "{prefix}#{name}|{variant}={}{units}",
                value.as_f64()
            ))
        } else {
            self.0.push(format_args!(
                "{prefix}#{name}|{variant}={}{units}",
                value.as_u64()
            ))
        }
    }
}

impl super::BoolRecorder for Recorder {
    fn record(&self, info: &'static super::Info, value: bool) {
        let prefix = self.1;
        let name = info.name;
        let v = format!("{prefix}#{name}={value}");
        self.0.push(v);
    }
}
