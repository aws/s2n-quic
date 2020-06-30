use crate::timer::{
    entry::TimerEntry,
    shared_state::{TimerNode, TimerSharedState},
};
use alloc::rc::Rc;
use core::cell::RefCell;
use s2n_quic_core::time::Timestamp;

/// `TimerManager` allows to create [`Timer`]s. Each [`Timer`] can set an individual
/// timeout. `TimerManager` allows to query for the next timeout.
pub struct TimerManager<T> {
    /// The shared state between timer and client
    state: Rc<RefCell<TimerSharedState<T>>>,
}

impl<T: Copy> TimerManager<T> {
    /// Create a new TimerManager
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(TimerSharedState::new())),
        }
    }

    /// Creates a timer.
    ///
    /// The timer will initially be configured to expire
    /// at the provided [`Timestamp`].
    pub fn create_timer(&self, timer_token: T, expiration: Timestamp) -> TimerEntry<T> {
        let timer_node = Rc::new(TimerNode::new_active(timer_token, expiration));

        // Timers are created in Active state. Therefore they must be
        // inserted into the timers list immediately.
        let mut guard = self.state.borrow_mut();
        guard.timers.insert(timer_node.clone());
        drop(guard);

        TimerEntry::new(self.state.clone(), timer_node)
    }

    /// Returns the `Timestamp` when the next `Timer` that is managed by the
    /// `TimerManager` expires. This `Timestamp` might be in the past.
    pub fn next_expiration(&self) -> Option<Timestamp> {
        self.state.borrow().next_expiration()
    }

    /// Checks whether a timer expired based on the current time - which gets
    /// provided as a [`Timestamp`].
    /// If a Timer expired, the token which identifies the timer gets returned.
    pub fn pop_expired(&mut self, current_time: Timestamp) -> Option<T> {
        self.state.borrow_mut().pop_expired(current_time)
    }

    /// Returns an iterator over expired timers, based on the given [`Timestamp`]
    pub fn expirations<'a>(&'a mut self, current_time: Timestamp) -> impl Iterator<Item = T> + 'a {
        TimerExpirationIterator {
            timer_manager: self,
            current_time,
        }
    }
}

/// An iterator over expired timers
struct TimerExpirationIterator<'a, T> {
    timer_manager: &'a mut TimerManager<T>,
    current_time: Timestamp,
}

impl<'a, T: Copy> Iterator for TimerExpirationIterator<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.timer_manager.pop_expired(self.current_time)
    }
}
