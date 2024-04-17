pub trait Controller {
    /// Returns the source port to which control/reset messages should be sent
    fn source_port(&self) -> u16;
}

impl Controller for u16 {
    #[inline]
    fn source_port(&self) -> u16 {
        *self
    }
}
