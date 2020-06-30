//! An unbounded multi-producer single-consumer channel

use alloc::{collections::VecDeque, sync::Arc};
use core::task::{Context, Poll, Waker};
use std::sync::Mutex;

/// Shared state of the chnanel
struct ChannelState<T> {
    /// Queued elements
    queue: VecDeque<T>,
    /// The task which should be notified if a new element was queued
    waker: Option<Waker>,
}

impl<T> ChannelState<T> {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            waker: None,
        }
    }
}

/// Creates an unbounded channel
///
/// The method will return a pair of a [`Sender`] and an [`Receiver`].
///
/// Publishing an item to the [`Sender`] will allow another task to retrieve them
/// through the [`Receiver`].
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let state = Arc::new(Mutex::new(ChannelState::new()));
    let sender = Sender {
        state: state.clone(),
    };
    let receveiver = Receiver { state };
    (sender, receveiver)
}

/// The [`Sender`] allows to store items in a channel, which can then be retrieved
/// by the [`Receiver`].
#[derive(Clone)]
pub struct Sender<T> {
    state: Arc<Mutex<ChannelState<T>>>,
}

impl<T> Sender<T> {
    /// Send `item` via the channel.
    pub fn send(&mut self, item: T) {
        let waker = {
            let guard = &mut *self.state.lock().expect("Locks can only fail if poisoned");
            guard.queue.push_back(item);
            guard.waker.take()
        };

        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// The [`Receiver`] allows to dequeue elements from the channel
pub struct Receiver<T> {
    state: Arc<Mutex<ChannelState<T>>>,
}

impl<T> Receiver<T> {
    /// Polls for next received item and returns it if available.
    pub fn poll_next(&mut self, context: &Context) -> Poll<T> {
        let mut guard = self.state.lock().expect("Locks can only fail if poisoned");

        if let Some(item) = guard.queue.pop_front() {
            Poll::Ready(item)
        } else {
            // We only need to replace the Waker if there is none stored yet
            // which will do the same job.
            match &mut guard.waker {
                Some(current_waker) if current_waker.will_wake(context.waker()) => Poll::Pending,
                _ => {
                    guard.waker = Some(context.waker().clone());
                    Poll::Pending
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_test::task::new_count_waker;

    #[test]
    fn test_unbounded_channel() {
        let (waker, wake_counter) = new_count_waker();

        let (mut sender, mut receiver) = channel();

        assert_eq!(
            Poll::Pending,
            receiver.poll_next(&Context::from_waker(&waker))
        );
        sender.send(5);

        assert_eq!(wake_counter, 1);
        assert_eq!(
            Poll::Ready(5),
            receiver.poll_next(&Context::from_waker(&waker))
        );
        assert_eq!(wake_counter, 1);
        assert_eq!(
            Poll::Pending,
            receiver.poll_next(&Context::from_waker(&waker))
        );

        let mut cloned_sender = sender.clone();
        cloned_sender.send(7);
        assert_eq!(wake_counter, 2);
        assert_eq!(
            Poll::Ready(7),
            receiver.poll_next(&Context::from_waker(&waker))
        );
        assert_eq!(wake_counter, 2);
        assert_eq!(
            Poll::Pending,
            receiver.poll_next(&Context::from_waker(&waker))
        );

        sender.send(52);
        assert_eq!(wake_counter, 3);
        assert_eq!(
            Poll::Ready(52),
            receiver.poll_next(&Context::from_waker(&waker))
        );
        assert_eq!(wake_counter, 3);
        assert_eq!(
            Poll::Pending,
            receiver.poll_next(&Context::from_waker(&waker))
        );
    }
}
