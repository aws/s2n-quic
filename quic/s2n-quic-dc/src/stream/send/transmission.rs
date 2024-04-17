#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    Probe,
    Stream,
}

impl Type {
    #[inline]
    pub fn is_probe(self) -> bool {
        matches!(self, Self::Probe)
    }

    #[inline]
    pub fn is_stream(self) -> bool {
        matches!(self, Self::Stream)
    }
}
