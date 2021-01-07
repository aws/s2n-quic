use crate::stream::Data;
use bolero_generator::{gen, TypeGenerator, ValueGenerator};
use core::time::Duration;

pub use std::collections::BTreeMap as Map;

#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Eq, Ord, TypeGenerator)]
pub struct Scenario {
    /// The streams owned by the client
    pub client: Streams,
    /// The streams owned by the server
    pub server: Streams,
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, TypeGenerator)]
pub struct Streams {
    /// The locally-owned unidirectional streams
    #[generator(gen::<Map<u64, UniStream>>().with().len(0usize..=25))]
    pub uni_streams: Map<u64, UniStream>,
    /// The locally-owned bidirectional streams
    #[generator(gen::<Map<u64, BidiStream>>().with().len(0usize..=25))]
    pub bidi_streams: Map<u64, BidiStream>,
}

impl Default for Streams {
    fn default() -> Self {
        Self {
            uni_streams: Iterator::map(1..=25, |id| (id, Default::default())).collect(),
            bidi_streams: Iterator::map(1..=25, |id| (id, Default::default())).collect(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord, TypeGenerator)]
pub struct UniStream {
    /// The amount of time the initiator should delay before opening the stream
    #[generator((0..=2).map_gen(Duration::from_millis))]
    pub delay: Duration,
    /// The stream data that should be sent from the local (initiator) towards the peer
    pub local: Stream,
}

impl Default for UniStream {
    fn default() -> Self {
        Self {
            delay: Duration::default(),
            local: Stream::default(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord, TypeGenerator)]
pub struct BidiStream {
    /// The amount of time the initiator should delay before opening the stream
    #[generator((0..=2).map_gen(Duration::from_millis))]
    pub delay: Duration,
    /// The stream data that should be sent from the local (initiator) towards the peer
    pub local: Stream,
    /// The stream data that should be sent from the peer (non-initiator) towards the initiator
    pub peer: Stream,
}

impl Default for BidiStream {
    fn default() -> Self {
        Self {
            delay: Duration::default(),
            local: Stream::default(),
            peer: Stream::default(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord, TypeGenerator)]
pub struct Stream {
    /// The data that should be sent over the stream
    pub data: Data,
    /// A potential error that could happen on the sending side
    pub reset: Option<Error>,
    /// A potential error that could happen on the receving side
    pub stop_sending: Option<Error>,
    /// The size of the chunks that should be sent on the stream
    pub send_amount: SendAmount,
}

impl Default for Stream {
    fn default() -> Self {
        Self {
            data: Data::default(),
            reset: None,
            stop_sending: None,
            send_amount: SendAmount::default(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord, TypeGenerator)]
pub struct Error {
    /// The offset at which this error should happen
    pub offset: usize,
    /// The code of the error
    pub code: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub struct SendAmount {
    /// The minimal amount of data that should be sent in a chunk
    pub min: usize,
    /// The maximum amount of data that should be sent in a chunk
    pub max: usize,
}

impl Default for SendAmount {
    fn default() -> Self {
        Self { min: 32, max: 256 }
    }
}

impl TypeGenerator for SendAmount {
    fn generate<D: bolero_generator::driver::Driver>(driver: &mut D) -> Option<Self> {
        let min = bolero_generator::ValueGenerator::generate(&(1..=2048), driver)?;
        let variance = bolero_generator::ValueGenerator::generate(&(0..=1024), driver)?;
        let max = min + variance;
        Some(Self { min, max })
    }
}

impl SendAmount {
    pub fn iter(&self) -> impl Iterator<Item = usize> {
        let min = self.min.min(self.max);
        let max = self.min.max(self.max);

        Iterator::map(min..=max, |amount| {
            // ensure we send at least 1 byte otherwise we'll endlessly loop
            amount.max(1)
        })
        .cycle()
    }
}
