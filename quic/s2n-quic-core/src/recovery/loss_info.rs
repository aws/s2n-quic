use crate::time::Duration;

#[must_use = "Ignoring loss information would lead to permanent data loss"]
#[derive(Copy, Clone, Default)]
pub struct LossInfo {
    /// Lost bytes in flight
    pub bytes_in_flight: usize,

    /// A PTO timer expired
    pub pto_expired: bool,

    /// The PTO count should be reset
    pub pto_reset: bool,

    /// The longest period of persistent congestion
    pub persistent_congestion_period: Duration,
}

impl LossInfo {
    /// The recovery manager requires updating if a PTO expired/needs to be reset, or
    /// loss packets were detected.
    pub fn updated_required(&self) -> bool {
        self.bytes_in_flight > 0 || self.pto_expired || self.pto_reset
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl core::ops::Add for LossInfo {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            bytes_in_flight: self.bytes_in_flight + rhs.bytes_in_flight,
            pto_expired: self.pto_expired || rhs.pto_expired,
            pto_reset: self.pto_reset || rhs.pto_reset,
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
