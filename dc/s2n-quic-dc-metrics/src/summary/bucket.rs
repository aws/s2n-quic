// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// This imports the bucketing algorithm from the `histogram` crate.
//
// See https://github.com/pelikan-io/rustcommon/tree/main/histogram for source.
//
// We made modifications to this file to better serves our needs.
//
// This is licensed as MIT or Apache 2.0.

/// The configuration of a histogram which determines the bucketing strategy and
/// therefore the relative error and memory utilization of a histogram.
/// * `grouping_power` - controls the number of buckets that are used to span
///   consecutive powers of two. Lower values result in less memory usage since
///   fewer buckets will be created. However, this will result in larger
///   relative error as each bucket represents a wider range of values.
/// * `max_value_power` - controls the largest value which can be stored in the
///   histogram. `2^(max_value_power) - 1` is the inclusive upper bound for the
///   representable range of values.
///
/// # How to choose parameters for your data
/// Please see <https://observablehq.com/@iopsystems/h2histogram> for an
/// in-depth discussion about the bucketing strategy and an interactive
/// calculator that lets you explore how these parameters result in histograms
/// with varying error guarantees and memory utilization requirements.
///
/// # The short version
/// ## Grouping Power
/// `grouping_power` should be set such that `2^(-1 * grouping_power)` is an
/// acceptable relative error. Rephrased, we can plug-in the acceptable
/// relative error into `grouping_power = ceil(log2(1/e))`. For example, if we
/// want to limit the error to 0.1% (0.001) we should set `grouping_power = 7`.
///
/// ## Max Value Power
/// `max_value_power` should be the closest power of 2 that is larger than the
/// largest value you expect in your data. If your only guarantee is that the
/// values are all `u64`, then setting this to `64` may be reasonable if you
/// can tolerate a bit of relative error.
///
/// ## Resulting size
///
/// If we want to allow any value in a range of unsigned types, the amount of
/// memory for the histogram is approximately:
///
/// | power | error |     u16 |     u32 |     u64 |
/// |-------|-------|---------|---------|---------|
/// |     2 |   25% | 0.6 KiB |   1 KiB |   2 KiB |
/// |     3 | 12.5% |   1 KiB |   2 KiB |   4 KiB |
/// |     4 | 6.25% |   2 KiB |   4 KiB |   8 KiB |
/// |     5 | 3.13% |   3 KiB |   7 KiB |  15 KiB |
/// |     6 | 1.56% |   6 KiB |  14 KiB |  30 KiB |
/// |     7 | .781% |  10 KiB |  26 KiB |  58 KiB |
/// |     8 | .391% |  18 KiB |  50 KiB | 114 KiB |
/// |     9 | .195% |  32 KiB |  96 KiB | 224 KiB |
/// |    10 | .098% |  56 KiB | 184 KiB | 440 KiB |
/// |    11 | .049% |  96 KiB | 352 KiB | 864 KiB |
/// |    12 | .025% | 160 KiB | 672 KiB | 1.7 MiB |
///
/// # Constraints:
/// * `max_value_power` must be in the range `0..=64`
/// * `max_value_power` must be greater than `grouping_power
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    max: u64,
    grouping_power: u8,
    max_value_power: u8,
    cutoff_power: u8,
    cutoff_value: u64,
    lower_bin_count: u32,
    upper_bin_divisions: u32,
    upper_bin_count: u32,
}

impl Config {
    /// Create a new histogram `Config` from the parameters. See the struct
    /// documentation [`crate::Config`] for the meaning of the parameters and
    /// their constraints.
    pub const fn new(grouping_power: u8, max_value_power: u8) -> Self {
        // we only allow values up to 2^64
        if max_value_power > 64 {
            panic!("max power too high");
        }

        // check that the other parameters make sense together
        if grouping_power >= max_value_power {
            panic!("max power too low");
        }

        // the cutoff is the point at which the linear range divisions and the
        // logarithmic range subdivisions diverge.
        //
        // for example:
        // when a = 0, the linear range has bins with width 1.
        // if b = 7 the logarithmic range has 128 subdivisions.
        // this means that for 0..128 we must be representing the values exactly
        // but we also represent 128..256 exactly since the subdivisions divide
        // that range into bins with the same width as the linear portion.
        //
        // therefore our cutoff power = a + b + 1

        // note: because a + b must be less than n which is a u8, a + b + 1 must
        // be less than or equal to u8::MAX. This means our cutoff power will
        // always fit in a u8
        let cutoff_power = grouping_power + 1;
        let cutoff_value = 2_u64.pow(cutoff_power as u32);
        let lower_bin_width = 2_u32.pow(0);
        let upper_bin_divisions = 2_u32.pow(grouping_power as u32);

        let max = if max_value_power == 64 {
            u64::MAX
        } else {
            2_u64.pow(max_value_power as u32)
        };

        let lower_bin_count = (cutoff_value / lower_bin_width as u64) as u32;
        let upper_bin_count = (max_value_power - cutoff_power) as u32 * upper_bin_divisions;

        Self {
            max,
            grouping_power,
            max_value_power,
            cutoff_power,
            cutoff_value,
            lower_bin_count,
            upper_bin_divisions,
            upper_bin_count,
        }
    }

    /// Returns the grouping power that was used to create this configuration.
    pub const fn grouping_power(&self) -> u8 {
        self.grouping_power
    }

    /// Returns the max value power that was used to create this configuration.
    pub const fn max_value_power(&self) -> u8 {
        self.max_value_power
    }

    /// Return the total number of buckets needed for this config.
    pub const fn total_buckets(&self) -> usize {
        (self.lower_bin_count + self.upper_bin_count) as usize
    }

    /// Converts a value to a bucket index. Returns an error if the value is
    /// outside of the range for the config.
    pub(crate) fn value_to_index(&self, value: u64) -> Option<usize> {
        if value < self.cutoff_value {
            return Some(value as usize);
        }

        if value > self.max {
            return None;
        }

        let power = 63 - value.leading_zeros();
        let log_bin = power - self.cutoff_power as u32;
        let offset = (value - (1 << power)) >> (power - self.grouping_power as u32);

        Some((self.lower_bin_count + log_bin * self.upper_bin_divisions + offset as u32) as usize)
    }

    /// Convert a bucket index to a lower bound.
    pub(crate) fn index_to_lower_bound(&self, index: usize) -> u64 {
        let g = index as u64 >> self.grouping_power;
        let h = index as u64 - g * (1 << self.grouping_power);

        if g < 1 {
            h
        } else {
            (1 << (self.grouping_power as u64 + g - 1)) + (1 << (g - 1)) * h
        }
    }

    /// Convert a bucket index to a upper inclusive bound.
    pub(crate) fn index_to_upper_bound(&self, index: usize) -> u64 {
        if index as u32 == self.lower_bin_count + self.upper_bin_count - 1 {
            return self.max;
        }
        let g = index as u64 >> self.grouping_power;
        let h = index as u64 - g * (1 << self.grouping_power) + 1;

        if g < 1 {
            h - 1
        } else {
            (1 << (self.grouping_power as u64 + g - 1)) + (1 << (g - 1)) * h - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn sizes() {
        assert_eq!(std::mem::size_of::<Config>(), 32);
    }

    #[test]
    // Test that the number of buckets matches the expected count
    fn total_buckets() {
        let config = Config::new(2, 64);
        assert_eq!(config.total_buckets(), 252);

        let config = Config::new(7, 64);
        assert_eq!(config.total_buckets(), 7424);

        let config = Config::new(14, 64);
        assert_eq!(config.total_buckets(), 835_584);

        let config = Config::new(2, 4);
        assert_eq!(config.total_buckets(), 12);
    }

    #[test]
    // Test value to index conversions
    fn value_to_idx() {
        let config = Config::new(7, 64);
        assert_eq!(config.value_to_index(0), Some(0));
        assert_eq!(config.value_to_index(1), Some(1));
        assert_eq!(config.value_to_index(256), Some(256));
        assert_eq!(config.value_to_index(257), Some(256));
        assert_eq!(config.value_to_index(258), Some(257));
        assert_eq!(config.value_to_index(512), Some(384));
        assert_eq!(config.value_to_index(515), Some(384));
        assert_eq!(config.value_to_index(516), Some(385));
        assert_eq!(config.value_to_index(1024), Some(512));
        assert_eq!(config.value_to_index(1031), Some(512));
        assert_eq!(config.value_to_index(1032), Some(513));
        assert_eq!(config.value_to_index(u64::MAX - 1), Some(7423));
        assert_eq!(config.value_to_index(u64::MAX), Some(7423));
    }

    #[test]
    // Test index to lower bound conversion
    fn idx_to_lower_bound() {
        let config = Config::new(7, 64);
        assert_eq!(config.index_to_lower_bound(0), 0);
        assert_eq!(config.index_to_lower_bound(1), 1);
        assert_eq!(config.index_to_lower_bound(256), 256);
        assert_eq!(config.index_to_lower_bound(384), 512);
        assert_eq!(config.index_to_lower_bound(512), 1024);
        assert_eq!(
            config.index_to_lower_bound(7423),
            18_374_686_479_671_623_680
        );
    }

    #[test]
    // Test index to upper bound conversion
    fn idx_to_upper_bound() {
        let config = Config::new(7, 64);
        assert_eq!(config.index_to_upper_bound(0), 0);
        assert_eq!(config.index_to_upper_bound(1), 1);
        assert_eq!(config.index_to_upper_bound(256), 257);
        assert_eq!(config.index_to_upper_bound(384), 515);
        assert_eq!(config.index_to_upper_bound(512), 1031);
        assert_eq!(config.index_to_upper_bound(7423), u64::MAX);
    }
}
