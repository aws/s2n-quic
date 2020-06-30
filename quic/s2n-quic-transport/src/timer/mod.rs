//! The timer module provides a mechanism that allows components to register
//! timers.

mod entry;
mod manager;
mod shared_state;

pub use entry::TimerEntry;
pub use manager::TimerManager;
pub use s2n_quic_core::time::Timer as VirtualTimer;

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    #[test]
    fn timer_test() {
        let mut manager = TimerManager::new();

        assert_eq!(None, manager.next_expiration());

        let time = s2n_quic_platform::time::now();

        let timer_id1 = 1u64;
        let expiration1 = time + Duration::from_millis(100);
        let _timer1 = manager.create_timer(timer_id1, expiration1);
        assert_eq!(Some(expiration1), manager.next_expiration());

        let timer_id2 = 2u64;
        let expiration2 = time + Duration::from_millis(200);
        let _timer2 = manager.create_timer(timer_id2, expiration2);
        assert_eq!(Some(expiration1), manager.next_expiration());

        let timer_id3 = 3u64;
        let expiration3 = time + Duration::from_millis(50);
        // Explicitly reset the timer before it is checked for expiration
        let mut timer3 = manager.create_timer(timer_id3, time);
        timer3.update(Some(expiration3));
        assert_eq!(Some(expiration3), manager.next_expiration());

        // Remove the timer
        timer3.update(None);
        assert_eq!(Some(expiration1), manager.next_expiration());
        timer3.update(None);
        assert_eq!(Some(expiration1), manager.next_expiration());

        // Adjust the timer
        let expiration4 = time + Duration::from_millis(70);
        timer3.update(Some(expiration4));
        assert_eq!(Some(expiration4), manager.next_expiration());

        assert_eq!(None, manager.pop_expired(time + Duration::from_millis(50)));

        assert_eq!(Some(timer_id3), manager.pop_expired(expiration4));
        assert_eq!(None, manager.pop_expired(expiration4));

        let expiration5 = time + Duration::from_millis(150);
        timer3.update(Some(expiration5));

        let expiration6 = time + Duration::from_millis(200);

        let expired: Vec<_> = manager.expirations(expiration6).collect();
        assert_eq!(vec![timer_id1, timer_id3, timer_id2], expired);
    }

    #[test]
    fn unregister_timer_on_drop() {
        let mut manager = TimerManager::new();

        assert_eq!(None, manager.next_expiration());

        let time = s2n_quic_platform::time::now();

        let timer_id1 = 1u64;
        let expiration1 = time + Duration::from_millis(100);
        let timer1 = manager.create_timer(timer_id1, expiration1);
        assert_eq!(Some(expiration1), manager.next_expiration());

        drop(timer1);

        assert_eq!(None, manager.pop_expired(expiration1));
    }
}
