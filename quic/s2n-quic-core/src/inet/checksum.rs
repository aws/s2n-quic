// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{fmt, hash::Hasher, num::Wrapping};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod x86;

/// Computes the [IP checksum](https://www.rfc-editor.org/rfc/rfc1071) over the given slice of bytes
#[inline]
pub fn checksum(data: &[u8]) -> u16 {
    let mut checksum = Checksum::default();
    checksum.write(data);
    checksum.finish()
}

/// Minimum size for a payload to be considered for platform-specific code
const LARGE_WRITE_LEN: usize = 32;

/// Platform-specific function for computing a checksum
type LargeWriteFn = for<'a> unsafe fn(&mut Wrapping<u32>, bytes: &'a [u8]) -> &'a [u8];

/// Generic implementation of a function that computes a checksum over the given slice
#[inline]
fn write_sized_generic<'a, const LEN: usize>(
    state: &mut Wrapping<u32>,
    mut bytes: &'a [u8],
) -> &'a [u8] {
    //= https://www.rfc-editor.org/rfc/rfc1071#section-4.1
    //# The following "C" code algorithm computes the checksum with an inner
    //# loop that sums 16-bits at a time in a 32-bit accumulator.
    //#
    //# in 6
    //#    {
    //#        /* Compute Internet Checksum for "count" bytes
    //#         *         beginning at location "addr".
    //#         */
    //#    register long sum = 0;
    //#
    //#     while( count > 1 )  {
    //#        /*  This is the inner loop */
    //#            sum += * (unsigned short) addr++;
    //#            count -= 2;
    //#    }
    //#
    //#        /*  Add left-over byte, if any */
    //#    if( count > 0 )
    //#            sum += * (unsigned char *) addr;
    //#
    //#        /*  Fold 32-bit sum to 16 bits */
    //#    while (sum>>16)
    //#        sum = (sum & 0xffff) + (sum >> 16);
    //#
    //#    checksum = ~sum;
    //# }

    while bytes.len() >= LEN {
        let (chunks, remaining) = bytes.split_at(LEN);

        bytes = remaining;

        let mut sum = 0;
        // for each pair of bytes, interpret them as 16 bit integers and sum them up
        for chunk in chunks.chunks_exact(2) {
            let value = u16::from_ne_bytes([chunk[0], chunk[1]]) as u32;
            sum += value;
        }
        *state += sum;
    }

    bytes
}

/// Returns the most optimized function implementation for the current platform
#[inline]
#[cfg(all(feature = "once_cell", not(any(kani, miri))))]
fn probe_write_large() -> LargeWriteFn {
    static LARGE_WRITE_FN: once_cell::sync::Lazy<LargeWriteFn> = once_cell::sync::Lazy::new(|| {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if let Some(fun) = x86::probe() {
                return fun;
            }
        }

        write_sized_generic::<8>
    });

    *LARGE_WRITE_FN
}

#[inline]
#[cfg(not(all(feature = "once_cell", not(any(kani, miri)))))]
fn probe_write_large() -> LargeWriteFn {
    write_sized_generic::<8>
}

/// Computes the [IP checksum](https://www.rfc-editor.org/rfc/rfc1071) over an arbitrary set of inputs
#[derive(Clone, Copy)]
pub struct Checksum {
    state: Wrapping<u32>,
    partial_write: bool,
    write_large: LargeWriteFn,
}

impl Default for Checksum {
    fn default() -> Self {
        Self {
            state: Default::default(),
            partial_write: false,
            write_large: probe_write_large(),
        }
    }
}

impl fmt::Debug for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut v = *self;
        v.carry();
        f.debug_tuple("Checksum").field(&v.finish()).finish()
    }
}

impl Checksum {
    /// Writes a single byte to the checksum state
    #[inline]
    fn write_byte(&mut self, byte: u8, shift: bool) {
        if shift {
            self.state += (byte as u32) << 8;
        } else {
            self.state += byte as u32;
        }
    }

    /// Carries all of the bits into a single 16 bit range
    #[inline]
    fn carry(&mut self) {
        let mut state = self.state;

        for _ in 0..3 {
            state = Wrapping((state.0 & 0xffff) + (state.0 >> 16));
        }

        self.state = state;
    }

    /// Writes bytes to the checksum and ensures any single byte remainders are padded
    #[inline]
    pub fn write_padded(&mut self, bytes: &[u8]) {
        self.write(bytes);

        // write a null byte if `bytes` wasn't 16-bit aligned
        if core::mem::take(&mut self.partial_write) {
            self.write_byte(0, cfg!(target_endian = "little"));
        }
    }

    /// Computes the final checksum
    #[inline]
    pub fn finish(mut self) -> u16 {
        self.carry();

        let value = self.state.0 as u16;
        let value = !value;

        // if value is 0, we need to set it to the max value to indicate the checksum was actually
        // computed
        if value == 0 {
            return 0xffff;
        }

        value.to_be()
    }
}

impl Hasher for Checksum {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        // Check to see if we have a partial write to flush
        if core::mem::take(&mut self.partial_write) {
            let (chunk, remaining) = bytes.split_at(1);
            bytes = remaining;

            // shift the byte if we're on little endian
            self.write_byte(chunk[0], cfg!(target_endian = "little"));
        }

        // Only delegate to the optimized platform function if the payload is big enough
        if bytes.len() >= LARGE_WRITE_LEN {
            bytes = unsafe { (self.write_large)(&mut self.state, bytes) };
        }

        // Fall back on the generic implementation to wrap things up
        bytes = write_sized_generic::<2>(&mut self.state, bytes);

        // if we only have a single byte left, write it to the state and mark it as a partial write
        if let Some(byte) = bytes.first().copied() {
            self.partial_write = true;
            self.write_byte(byte, cfg!(target_endian = "big"));
        }
    }

    #[inline]
    fn finish(&self) -> u64 {
        Self::finish(*self) as _
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    #[test]
    fn rfc_example_test() {
        //= https://www.rfc-editor.org/rfc/rfc1071#section-3
        //= type=test
        //# We now present explicit examples of calculating a simple 1's
        //# complement sum on a 2's complement machine.  The examples show the
        //# same sum calculated byte by bye, by 16-bits words in normal and
        //# swapped order, and 32 bits at a time in 3 different orders.  All
        //# numbers are in hex.
        //#
        //#               Byte-by-byte    "Normal"  Swapped
        //#                                 Order    Order
        //#
        //#     Byte 0/1:    00   01        0001      0100
        //#     Byte 2/3:    f2   03        f203      03f2
        //#     Byte 4/5:    f4   f5        f4f5      f5f4
        //#     Byte 6/7:    f6   f7        f6f7      f7f6
        //#                 ---  ---       -----     -----
        //#     Sum1:       2dc  1f0       2ddf0     1f2dc
        //#
        //#                  dc   f0        ddf0      f2dc
        //#     Carrys:       1    2           2         1
        //#                  --   --        ----      ----
        //#     Sum2:        dd   f2        ddf2      f2dd
        //#
        //#     Final Swap:  dd   f2        ddf2      ddf2
        let bytes = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];

        let mut checksum = Checksum::default();
        checksum.write(&bytes);
        checksum.carry();

        assert_eq!((checksum.state.0 as u16).to_le_bytes(), [0xdd, 0xf2]);
        assert_eq!((!rfc_c_port(&bytes)).to_be_bytes(), [0xdd, 0xf2]);
    }

    fn rfc_c_port(data: &[u8]) -> u16 {
        //= https://www.rfc-editor.org/rfc/rfc1071#section-4.1
        //= type=test
        //# The following "C" code algorithm computes the checksum with an inner
        //# loop that sums 16-bits at a time in a 32-bit accumulator.
        //#
        //# in 6
        //#    {
        //#        /* Compute Internet Checksum for "count" bytes
        //#         *         beginning at location "addr".
        //#         */
        //#    register long sum = 0;
        //#
        //#     while( count > 1 )  {
        //#        /*  This is the inner loop */
        //#            sum += * (unsigned short) addr++;
        //#            count -= 2;
        //#    }
        //#
        //#        /*  Add left-over byte, if any */
        //#    if( count > 0 )
        //#            sum += * (unsigned char *) addr;
        //#
        //#        /*  Fold 32-bit sum to 16 bits */
        //#    while (sum>>16)
        //#        sum = (sum & 0xffff) + (sum >> 16);
        //#
        //#    checksum = ~sum;
        //# }

        let mut addr = data.as_ptr() as *const u8;
        let mut count = data.len();

        unsafe {
            let mut sum = 0u32;

            while count > 1 {
                let value = u16::from_be_bytes([*addr, *addr.add(1)]);
                sum = sum.wrapping_add(value as u32);
                addr = addr.add(2);
                count -= 2;
            }

            if count > 0 {
                let value = u16::from_be_bytes([*addr, 0]);
                sum = sum.wrapping_add(value as u32);
            }

            while sum >> 16 != 0 {
                sum = (sum & 0xffff) + (sum >> 16);
            }

            !(sum as u16)
        }
    }

    /// * Compares the implementation to a port of the C code defined in the RFC
    /// * Ensures partial writes are correctly handled, even if they're not at a 16 bit boundary
    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(8), kani::solver(kissat))]
    fn differential() {
        #[cfg(any(kani, miri))]
        type Bytes = crate::testing::InlineVec<u8, 5>;
        #[cfg(not(any(kani, miri)))]
        type Bytes = Vec<u8>;

        check!()
            .with_type::<(usize, Bytes)>()
            .for_each(|(index, bytes)| {
                let index = if bytes.is_empty() {
                    0
                } else {
                    *index % bytes.len()
                };
                let (a, b) = bytes.split_at(index);
                let mut cs = Checksum::default();
                cs.write(a);
                cs.write(b);

                let mut rfc_value = rfc_c_port(bytes);
                if rfc_value == 0 {
                    rfc_value = 0xffff;
                }

                assert_eq!(rfc_value.to_be_bytes(), cs.finish().to_be_bytes());
            });
    }
}
