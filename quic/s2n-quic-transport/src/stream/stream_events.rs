use core::task::Waker;

/// Events that are triggered through a state-update on a Stream.
/// E.g. retrieving new data can unblock readers, which is signalled via this
/// Event.
#[derive(Debug, Default)]
pub struct StreamEvents {
    /// A task that is waiting on the reading half of the Stream that might have
    /// been woken up due to an action.
    pub read_wake: Option<Waker>,
    /// A task that is waiting on the writing half of the Stream that might have
    /// been woken up due to an action.
    pub write_wake: Option<Waker>,
}

impl StreamEvents {
    /// Creates a new [`StreamEvents`] instance with no pending events
    pub fn new() -> StreamEvents {
        Default::default()
    }

    /// Returns the amount of [`Waker`] instances that are stored in the event
    pub fn waker_count(&self) -> usize {
        let mut result = 0;
        if self.read_wake.is_some() {
            result += 1;
        }
        if self.write_wake.is_some() {
            result += 1;
        }
        result
    }

    /// Stores a [`Waker`] which was waiting on the reading half of the Stream
    /// to get ready. The [`Waker`] will get woken up by the consumer of
    /// [`StreamEvents`].
    pub fn store_read_waker(&mut self, waker: Waker) {
        self.read_wake = Some(waker);
    }

    /// Stores a [`Waker`] which was waiting on the writing half of the Stream
    /// to get ready. The [`Waker`] will get woken up by the consumer of
    /// [`StreamEvents`].
    pub fn store_write_waker(&mut self, waker: Waker) {
        self.write_wake = Some(waker);
    }

    /// Wakes all [`Waker`] instances that have been stored via `add_waker`.
    /// Waking the [`Waker`]s will remove them from the [`DataEvent`]s struct.
    pub fn wake_all(&mut self) {
        if let Some(waker) = self.read_wake.take() {
            waker.wake();
        }
        if let Some(waker) = self.write_wake.take() {
            waker.wake();
        }
    }
}
