use crate::time::Duration;

#[derive(Copy, Clone, Default)]
pub struct LossInfo {}

#[allow(clippy::suspicious_arithmetic_impl)]
impl core::ops::Add for LossInfo {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {}
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
