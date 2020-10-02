use crate::time::Timestamp;

#[must_use = "Ignoring loss information would lead to permanent data loss"]
#[derive(Copy, Clone, Default)]
pub struct LossInfo {
    /// Lost bytes in flight
    pub bytes_in_flight: usize,

    /// A PTO timer expired
    pub pto_expired: bool,

    /// The PTO count should be reset
    pub pto_reset: bool,

    /// The time the lost packet with the largest packet number was sent
    pub largest_lost_packet_sent_time: Option<Timestamp>,
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
            largest_lost_packet_sent_time: self
                .largest_lost_packet_sent_time
                .iter()
                .chain(rhs.largest_lost_packet_sent_time.iter())
                .max()
                .copied(),
        }
    }
}

impl core::ops::AddAssign for LossInfo {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}
