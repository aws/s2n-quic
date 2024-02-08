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
