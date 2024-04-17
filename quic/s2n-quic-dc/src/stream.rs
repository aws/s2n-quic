use core::time::Duration;

pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

pub mod packet_map;
pub mod packet_number;
pub mod processing;
pub mod recv;
pub mod send;
