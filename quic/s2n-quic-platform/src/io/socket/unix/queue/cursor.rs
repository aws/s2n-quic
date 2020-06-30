use super::Segment;

/// Cursor for moving a message from one segment to the other; e.g. `ready` to `pending`.
#[derive(Debug)]
pub struct Cursor<'a> {
    /// Reference to the primary segment
    pub primary: &'a mut Segment,
    /// Reference to the primary segment
    pub secondary: &'a mut Segment,
}

impl<'a> Cursor<'a> {
    /// Creates a new `Cursor` with a primary and secondary `Segment`
    pub fn new(primary: &'a mut Segment, secondary: &'a mut Segment) -> Self {
        Self { primary, secondary }
    }

    /// Moves the message into the other segment
    pub fn finish(self) {
        self.primary.move_into(self.secondary, 1);
    }

    /// Preserves the message in the current segment
    pub fn cancel(self) {
        // noop
    }
}
