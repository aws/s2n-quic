// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// ensures each implementation returns a trailing chunk correctly
#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn trailing_chunk_test() {
    let mut dest = vec![];
    let mut source = vec![];
    bolero::check!()
        .with_type::<(u16, u16)>()
        .for_each(|(dest_len, source_len)| {
            source.resize(*source_len as usize, 42);

            let dest_len = (*source_len).min(*dest_len) as usize;
            dest.resize(dest_len, 0);
            let expected = &source[..dest_len];
            let dest = &mut dest[..];

            // direct implementation
            {
                let mut reader: &[u8] = &source[..];
                let mut target = &mut dest[..];

                let chunk = reader.partial_copy_into(&mut target).unwrap();
                assert_eq!(expected, &*chunk);
                assert!(
                    dest.iter().all(|b| *b == 0),
                    "no bytes should be copied into dest"
                );
            }

            // IoSlice implementation
            {
                let io_slice = [&source[..]];
                let mut reader = IoSlice::new(&io_slice);
                let mut target = &mut dest[..];

                let chunk = reader.partial_copy_into(&mut target).unwrap();
                assert_eq!(expected, &*chunk);
                assert!(
                    dest.iter().all(|b| *b == 0),
                    "no bytes should be copied into dest"
                );
            }

            // Buf implementation
            {
                let mut slice = &source[..];
                let mut reader = Buf::new(&mut slice);
                let mut target = &mut dest[..];

                let chunk = reader.partial_copy_into(&mut target).unwrap();
                assert_eq!(expected, &*chunk);
                assert!(
                    dest.iter().all(|b| *b == 0),
                    "no bytes should be copied into dest"
                );
            }

            // full_copy
            {
                let mut source = &source[..];
                let mut reader = source.full_copy();
                let mut target = &mut dest[..];

                let chunk = reader.partial_copy_into(&mut target).unwrap();
                assert!(chunk.is_empty());
                assert_eq!(expected, dest);
                dest.fill(0);
            }
        });
}

/// ensures each storage type correctly copies multiple chunks into the destination
#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn io_slice_test() {
    let mut dest = vec![];
    let mut source: Vec<Vec<u8>> = vec![];
    let mut pool = vec![];
    let mut expected = vec![];
    bolero::check!()
        .with_type::<(u16, Vec<u16>)>()
        .for_each(|(dest_len, source_lens)| {
            while source.len() > source_lens.len() {
                pool.push(source.pop().unwrap());
            }

            while source.len() < source_lens.len() {
                source.push(pool.pop().unwrap_or_default());
            }

            let mut source_len = 0;
            let mut last_chunk_idx = 0;
            let mut last_chunk_len = 0;
            let mut remaining_len = *dest_len as usize;
            for (idx, (len, source)) in source_lens.iter().zip(&mut source).enumerate() {
                let fill = (idx + 1) as u8;
                let len = *len as usize;
                source.resize(len, fill);
                source.fill(fill);
                if len > 0 && remaining_len > 0 {
                    last_chunk_idx = idx;
                    last_chunk_len = len.min(remaining_len);
                }
                source_len += len;
                remaining_len = remaining_len.saturating_sub(len);
            }

            let dest_len = source_len.min(*dest_len as usize);
            dest.resize(dest_len, 0);
            dest.fill(0);
            let dest = &mut dest[..];

            expected.resize(dest_len, 0);
            expected.fill(0);

            {
                // don't copy the last chunk, since that should be returned
                let source = &source[..last_chunk_idx];
                crate::slice::vectored_copy(source, &mut [&mut expected[..]]);
            }

            let expected_chunk = source
                .get(last_chunk_idx)
                .map(|v| &v[..last_chunk_len])
                .unwrap_or(&[]);

            // IoSlice implementation
            {
                let mut source = IoSlice::new(&source);
                let mut target = &mut dest[..];

                let chunk = source.partial_copy_into(&mut target).unwrap();

                assert_eq!(expected, dest);
                assert_eq!(expected_chunk, &*chunk);
                // reset the destination
                dest.fill(0);
            }

            // Buf implementation
            {
                let mut source = IoSlice::new(&source);
                let mut source = Buf::new(&mut source);
                let mut target = &mut dest[..];

                let chunk = source.partial_copy_into(&mut target).unwrap();

                assert_eq!(expected, dest);
                assert_eq!(expected_chunk, &*chunk);
            }
        });
}
