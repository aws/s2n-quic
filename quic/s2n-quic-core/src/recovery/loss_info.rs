use crate::time::Duration;

#[must_use = "Ignoring loss information would lead to permanent data loss"]
#[derive(Copy, Clone, Default)]
pub struct LossInfo {
    /// Lost bytes in flight
    pub bytes_in_flight: usize,

    /// The longest period of persistent congestion
    pub persistent_congestion_period: Duration,
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl core::ops::Add for LossInfo {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            bytes_in_flight: self.bytes_in_flight + rhs.bytes_in_flight,
            persistent_congestion_period: self
                .persistent_congestion_period
                .max(rhs.persistent_congestion_period),
        }
    }
}

impl core::ops::AddAssign for LossInfo {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl core::iter::Sum for LossInfo {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut loss_info = Self::default();

        for item in iter {
            loss_info += item;
        }

        loss_info
    }
}
