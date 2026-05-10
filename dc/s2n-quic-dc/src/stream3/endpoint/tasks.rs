// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{ByteCost, Receiver, Sender},
};
use core::{
    future::poll_fn,
    task::{self, Poll},
};
use std::sync::Arc;

/// Routing key accessor for stream3 send-side load-balancing tasks.
pub trait PathSecretMapEntry {
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry>;
}

impl<T> PathSecretMapEntry for crate::intrusive_queue::Entry<T>
where
    T: PathSecretMapEntry,
{
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        (**self).path_secret_entry()
    }
}

impl PathSecretMapEntry for crate::stream3::frame::Frame {
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        &self.path_secret_entry
    }
}

/// Routes items to socket senders by using pick-two path scheduling from the path secret map
/// entry associated with each item.
pub async fn pick_two<T, R, S, Rand>(mut rx: R, mut senders: Vec<S>, random: Rand)
where
    T: ByteCost + PathSecretMapEntry,
    R: Receiver<T>,
    S: Sender<T>,
    Rand: Fn(usize) -> usize,
{
    loop {
        let Some(entry) = rx.recv().await else {
            break;
        };

        let bytes = entry.byte_cost();
        let mut slot = core::mem::MaybeUninit::new(entry);

        let sent = poll_fn(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &random)).await;

        if !sent {
            // SAFETY: `slot` is initialized above with `MaybeUninit::new(entry)` and only
            // consumed by successful send.
            unsafe { slot.assume_init_drop() };
            break;
        }

        rx.on_consumed(bytes);
    }
}

fn try_send_pick_two<T, S, Rand>(
    cx: &mut task::Context<'_>,
    slot: &mut core::mem::MaybeUninit<T>,
    senders: &mut Vec<S>,
    random: &Rand,
) -> Poll<bool>
where
    T: PathSecretMapEntry,
    S: Sender<T>,
    Rand: Fn(usize) -> usize,
{
    if senders.is_empty() {
        return Poll::Ready(false);
    }

    let chosen_idx = {
        // SAFETY: `slot` is initialized with `MaybeUninit::new(entry)` and remains
        // initialized until it is consumed by a successful `poll_send`.
        let value = unsafe { &*slot.as_ptr() };
        let picked = value
            .path_secret_entry()
            .pick_sender_by_next_transmission(random);
        debug_assert!(
            picked < senders.len(),
            "picked sender index out of bounds: picked={} senders={}",
            picked,
            senders.len()
        );
        if picked >= senders.len() {
            return Poll::Ready(false);
        }
        picked
    };

    match senders[chosen_idx].poll_send(cx, slot) {
        Poll::Ready(Ok(())) => Poll::Ready(true),
        Poll::Ready(Err(())) => Poll::Ready(false),
        Poll::Pending => {
            let len = senders.len();
            for offset in 1..len {
                let idx = (chosen_idx + offset) % len;
                match senders[idx].poll_send(cx, slot) {
                    Poll::Ready(Ok(())) => return Poll::Ready(true),
                    Poll::Ready(Err(())) => return Poll::Ready(false),
                    Poll::Pending => {}
                }
            }
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::secret::map::Entry as PathSecretEntry;
    use core::{future::Future, mem::MaybeUninit, task::Poll};
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    struct TestItem {
        path_secret_entry: Arc<PathSecretEntry>,
        byte_cost: u64,
        drop_counter: Arc<AtomicUsize>,
    }

    impl Drop for TestItem {
        fn drop(&mut self) {
            self.drop_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ByteCost for TestItem {
        fn byte_cost(&self) -> u64 {
            self.byte_cost
        }
    }

    impl PathSecretMapEntry for TestItem {
        fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
            &self.path_secret_entry
        }
    }

    #[derive(Clone, Copy)]
    enum SenderBehavior {
        Pending,
        ReadyOk,
        ReadyErr,
    }

    struct TestSender {
        behavior: SenderBehavior,
        calls: usize,
    }

    impl Sender<TestItem> for TestSender {
        fn poll_send(
            &mut self,
            _cx: &mut task::Context<'_>,
            value: &mut MaybeUninit<TestItem>,
        ) -> Poll<Result<(), ()>> {
            self.calls += 1;

            match self.behavior {
                SenderBehavior::Pending => Poll::Pending,
                SenderBehavior::ReadyOk => {
                    // SAFETY: successful send consumes the value.
                    unsafe { value.assume_init_drop() };
                    Poll::Ready(Ok(()))
                }
                SenderBehavior::ReadyErr => Poll::Ready(Err(())),
            }
        }
    }

    struct TestReceiver {
        values: VecDeque<TestItem>,
        consumed: u64,
    }

    impl Receiver<TestItem> for TestReceiver {
        fn poll_recv(&mut self, _cx: &mut task::Context<'_>) -> Poll<Option<TestItem>> {
            Poll::Ready(self.values.pop_front())
        }

        fn on_consumed(&mut self, bytes: u64) {
            self.consumed += bytes;
        }
    }

    fn test_path_secret_entry() -> Arc<PathSecretEntry> {
        let peer = "127.0.0.1:4433"
            .parse()
            .expect("failed to parse hardcoded loopback address 127.0.0.1:4433");
        PathSecretEntry::fake(peer, None)
    }

    fn new_test_item(
        path_secret_entry: Arc<PathSecretEntry>,
        drop_counter: Arc<AtomicUsize>,
    ) -> TestItem {
        TestItem {
            path_secret_entry,
            byte_cost: 123,
            drop_counter,
        }
    }

    fn with_noop_context<R>(f: impl FnOnce(&mut task::Context<'_>) -> R) -> R {
        let waker = s2n_quic_core::task::waker::noop();
        let mut cx = task::Context::from_waker(&waker);
        f(&mut cx)
    }

    #[test]
    fn selected_sender_is_polled_before_alternates() {
        let mut slot = MaybeUninit::new(new_test_item(
            test_path_secret_entry(),
            Arc::new(AtomicUsize::new(0)),
        ));
        let mut senders = vec![
            TestSender {
                behavior: SenderBehavior::ReadyOk,
                calls: 0,
            },
            TestSender {
                behavior: SenderBehavior::ReadyOk,
                calls: 0,
            },
        ];
        let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
        assert_eq!(result, Poll::Ready(true));
        assert_eq!(senders[0].calls, 1);
        assert_eq!(senders[1].calls, 0);
    }

    #[test]
    fn falls_back_to_alternate_sender_when_selected_sender_is_pending() {
        let mut slot = MaybeUninit::new(new_test_item(
            test_path_secret_entry(),
            Arc::new(AtomicUsize::new(0)),
        ));
        let mut senders = vec![
            TestSender {
                behavior: SenderBehavior::Pending,
                calls: 0,
            },
            TestSender {
                behavior: SenderBehavior::ReadyOk,
                calls: 0,
            },
        ];
        let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
        assert_eq!(result, Poll::Ready(true));
        assert_eq!(senders[0].calls, 1);
        assert_eq!(senders[1].calls, 1);
    }

    #[test]
    fn shuts_down_on_sender_error() {
        let mut slot = MaybeUninit::new(new_test_item(
            test_path_secret_entry(),
            Arc::new(AtomicUsize::new(0)),
        ));
        let mut senders = vec![
            TestSender {
                behavior: SenderBehavior::ReadyErr,
                calls: 0,
            },
            TestSender {
                behavior: SenderBehavior::ReadyOk,
                calls: 0,
            },
        ];
        let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
        assert_eq!(result, Poll::Ready(false));
        assert_eq!(senders[0].calls, 1);
        assert_eq!(senders[1].calls, 0);

        // SAFETY: `Err` keeps the value in slot and caller must drop it.
        unsafe { slot.assume_init_drop() };
    }

    #[test]
    fn pick_two_drops_unsent_entry_on_shutdown() {
        let drop_counter = Arc::new(AtomicUsize::new(0));
        let rx = TestReceiver {
            values: [new_test_item(test_path_secret_entry(), drop_counter.clone())].into(),
            consumed: 0,
        };
        let senders = vec![TestSender {
            behavior: SenderBehavior::ReadyErr,
            calls: 0,
        }];
        let mut fut = core::pin::pin!(pick_two(rx, senders, |_| 0));
        let result = with_noop_context(|cx| fut.as_mut().poll(cx));
        assert_eq!(result, Poll::Ready(()));
        assert_eq!(drop_counter.load(Ordering::Relaxed), 1);
    }
}
