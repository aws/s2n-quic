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

/// The initial capacity of incoming connections
///
/// The value should be large enough to avoid reallocating the queue when the application is slow
/// to accept incoming connections.
const INITIAL_CAPACITY: usize = 1024;

impl<T> ChannelState<T> {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::with_capacity(INITIAL_CAPACITY),
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
    let state = Arc::new(Mutex::new(Some(ChannelState::new())));
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
    state: Arc<Mutex<Option<ChannelState<T>>>>,
}

fn close<T>(state: &Arc<Mutex<Option<ChannelState<T>>>>) -> Option<Waker> {
    let mut state = state.lock().ok()?;
    let state = core::mem::replace(&mut *state, None)?;
    state.waker
}

impl<T> Sender<T> {
    /// Send `item` via the channel.
    pub fn send(&mut self, item: T) -> Result<(), Error> {
        let waker = {
            let guard = &mut *self.state.lock().map_err(|_| Error)?;
            let guard = guard.as_mut().ok_or(Error)?;
            guard.queue.push_back(item);
            guard.waker.take()
        };

        if let Some(waker) = waker {
            waker.wake();
        }

        Ok(())
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // If there are 2 references, this is the last sender and the channel should be closed
        if Arc::strong_count(&self.state) == 2 {
            if let Some(waker) = close(&self.state) {
                // notify the receiver the channel is closed
                waker.wake()
            }
        }
    }
}

/// The [`Receiver`] allows to dequeue elements from the channel
pub struct Receiver<T> {
    state: Arc<Mutex<Option<ChannelState<T>>>>,
}

impl<T> Receiver<T> {
    /// Polls for next received item and returns it if available.
    pub fn poll_next(&mut self, context: &Context) -> Poll<Result<T, Error>> {
        /// checks the shared state status
        macro_rules! check_close {
            ($guard:ident) => {
                if let Some(guard) = $guard.as_mut() {
                    guard
                } else {
                    return Poll::Ready(Err(Error));
                };
            };
        }

        let mut guard = self.state.lock().ok();
        // unwrap the poison status
        let guard = check_close!(guard);
        // unwrap the Option
        let guard = check_close!(guard);

        if let Some(item) = guard.queue.pop_front() {
            Poll::Ready(Ok(item))
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

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // there is only a single receiver so close the channel
        let _ = close(&self.state);
    }
}

/// An error that is returned when the other side of the channel has closed
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Error;

#[cfg(test)]
mod tests {
    use super::*;
    use futures_test::task::new_count_waker;

    #[test]
    fn unbounded_channel_test() {
        let (waker, wake_counter) = new_count_waker();

        let (mut sender, mut receiver) = channel();

        assert_eq!(
            Poll::Pending,
            receiver.poll_next(&Context::from_waker(&waker))
        );
        sender.send(5).unwrap();

        assert_eq!(wake_counter, 1);
        assert_eq!(
            Poll::Ready(Ok(5)),
            receiver.poll_next(&Context::from_waker(&waker))
        );
        assert_eq!(wake_counter, 1);
        assert_eq!(
            Poll::Pending,
            receiver.poll_next(&Context::from_waker(&waker))
        );

        let mut cloned_sender = sender.clone();
        cloned_sender.send(7).unwrap();
        drop(cloned_sender);

        assert_eq!(wake_counter, 2);
        assert_eq!(
            Poll::Ready(Ok(7)),
            receiver.poll_next(&Context::from_waker(&waker))
        );
        assert_eq!(wake_counter, 2);
        assert_eq!(
            Poll::Pending,
            receiver.poll_next(&Context::from_waker(&waker))
        );

        sender.send(52).unwrap();
        assert_eq!(wake_counter, 3);
        assert_eq!(
            Poll::Ready(Ok(52)),
            receiver.poll_next(&Context::from_waker(&waker))
        );
        assert_eq!(wake_counter, 3);
        assert_eq!(
            Poll::Pending,
            receiver.poll_next(&Context::from_waker(&waker))
        );

        drop(sender);

        assert_eq!(
            Poll::Ready(Err(Error)),
            receiver.poll_next(&Context::from_waker(&waker))
        );
    }

    #[test]
    fn receiver_close_test() {
        let (mut sender, receiver) = channel();
        drop(receiver);

        assert_eq!(Err(Error), sender.send(1));
    }
}
