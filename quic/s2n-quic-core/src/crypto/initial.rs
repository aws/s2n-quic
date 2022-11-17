// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto;
use hex_literal::hex;

/// Types for which are able to perform initial cryptography.
///
/// This marker trait ensures only Initial-level keys
/// are used with Initial packets. Any key misuses are
/// caught by the type system.
pub trait InitialKey: crypto::Key + Sized {
    type HeaderKey: crypto::HeaderKey;

    fn new_server(connection_id: &[u8]) -> (Self, Self::HeaderKey);
    fn new_client(connection_id: &[u8]) -> (Self, Self::HeaderKey);
}

/// Types for which are able to perform initial header cryptography.
///
/// This marker trait ensures only Initial-level header keys
/// are used with Initial packets. Any key misuses are
/// caught by the type system.
pub trait InitialHeaderKey: crypto::HeaderKey {}

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.2
//# initial_salt = 0x38762cf7f55934b34d179ae6a4c80cadccbb7f0a

pub const INITIAL_SALT: [u8; 20] = hex!("38762cf7f55934b34d179ae6a4c80cadccbb7f0a");

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.2
//# client_initial_secret = HKDF-Expand-Label(initial_secret,
//#                                           "client in", "",
//#                                           Hash.length)

pub const INITIAL_CLIENT_LABEL: [u8; 9] = *b"client in";

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.2
//# server_initial_secret = HKDF-Expand-Label(initial_secret,
//#                                           "server in", "",
//#                                           Hash.length)

pub const INITIAL_SERVER_LABEL: [u8; 9] = *b"server in";

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A
//# These packets use an 8-byte client-chosen Destination Connection ID
//# of 0x8394c8f03e515708.

pub const EXAMPLE_DCID: [u8; 8] = hex!("8394c8f03e515708");

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.1
//# client_initial_secret
//#     = HKDF-Expand-Label(initial_secret, "client in", "", 32)
//#     = c00cf151ca5be075ed0ebfb5c80323c4
//#       2d6b7db67881289af4008f1f6c357aea

pub const EXAMPLE_CLIENT_INITIAL_SECRET: [u8; 32] = hex!(
    "
    c00cf151ca5be075ed0ebfb5c80323c4
    2d6b7db67881289af4008f1f6c357aea
    "
);

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.1
//# server_initial_secret
//#     = HKDF-Expand-Label(initial_secret, "server in", "", 32)
//#     = 3c199828fd139efd216c155ad844cc81
//#       fb82fa8d7446fa7d78be803acdda951b

pub const EXAMPLE_SERVER_INITIAL_SECRET: [u8; 32] = hex!(
    "
    3c199828fd139efd216c155ad844cc81
    fb82fa8d7446fa7d78be803acdda951b
    "
);

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.2
//# The client sends an Initial packet.  The unprotected payload of this
//# packet contains the following CRYPTO frame, plus enough PADDING
//# frames to make an 1162-byte payload:
//#
//# 060040f1010000ed0303ebf8fa56f129 39b9584a3896472ec40bb863cfd3e868
//# 04fe3a47f06a2b69484c000004130113 02010000c000000010000e00000b6578
//# 616d706c652e636f6dff01000100000a 00080006001d00170018001000070005
//# 04616c706e0005000501000000000033 00260024001d00209370b2c9caa47fba
//# baf4559fedba753de171fa71f50f1ce1 5d43e994ec74d748002b000302030400
//# 0d0010000e0403050306030203080408 050806002d00020101001c0002400100
//# 3900320408ffffffffffffffff050480 00ffff07048000ffff08011001048000
//# 75300901100f088394c8f03e51570806 048000ffff

/// Example payload from <https://www.rfc-editor.org/rfc/rfc9001#appendix-A.2>
pub const EXAMPLE_CLIENT_INITIAL_PAYLOAD: [u8; 245] = hex!(
    "
   060040f1010000ed0303ebf8fa56f129 39b9584a3896472ec40bb863cfd3e868
   04fe3a47f06a2b69484c000004130113 02010000c000000010000e00000b6578
   616d706c652e636f6dff01000100000a 00080006001d00170018001000070005
   04616c706e0005000501000000000033 00260024001d00209370b2c9caa47fba
   baf4559fedba753de171fa71f50f1ce1 5d43e994ec74d748002b000302030400
   0d0010000e0403050306030203080408 050806002d00020101001c0002400100
   3900320408ffffffffffffffff050480 00ffff07048000ffff08011001048000
   75300901100f088394c8f03e51570806 048000ffff
    "
);

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.2
//# The unprotected header indicates a length of 1182 bytes: the 4-byte
//# packet number, 1162 bytes of frames, and the 16-byte authentication
//# tag.  The header includes the connection ID and a packet number of 2:
//#
//# c300000001088394c8f03e5157080000449e00000002

pub const EXAMPLE_CLIENT_INITIAL_HEADER: [u8; 22] =
    hex!("c300000001088394c8f03e5157080000449e00000002");

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.2
//# Protecting the payload produces output that is sampled for header
//# protection.  Because the header uses a 4-byte packet number encoding,
//# the first 16 bytes of the protected payload is sampled and then
//# applied to the header as follows:
//#
//# sample = d1b1c98dd7689fb8ec11d242b123dc9b
//#
//# mask = AES-ECB(hp, sample)[0..4]
//#      = 437b9aec36
//#
//# header[0] ^= mask[0] & 0x0f
//#      = c0
//# header[18..21] ^= mask[1..4]
//#      = 7b9aec34
//# header = c000000001088394c8f03e5157080000449e7b9aec34

#[test]
fn client_initial_protection_test() {
    let mask = hex!("437b9aec36");
    let unprotected_header = EXAMPLE_CLIENT_INITIAL_HEADER;
    let protected_header = hex!("c000000001088394c8f03e5157080000449e7b9aec34");
    let packet_tag = 0b11; // results in 4 byte packet number

    header_protection_test_helper(mask, &unprotected_header, &protected_header, packet_tag);
}

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.2
//# The resulting protected packet is:
//#
//# c000000001088394c8f03e5157080000 449e7b9aec34d1b1c98dd7689fb8ec11
//# d242b123dc9bd8bab936b47d92ec356c 0bab7df5976d27cd449f63300099f399
//# 1c260ec4c60d17b31f8429157bb35a12 82a643a8d2262cad67500cadb8e7378c
//# 8eb7539ec4d4905fed1bee1fc8aafba1 7c750e2c7ace01e6005f80fcb7df6212
//# 30c83711b39343fa028cea7f7fb5ff89 eac2308249a02252155e2347b63d58c5
//# 457afd84d05dfffdb20392844ae81215 4682e9cf012f9021a6f0be17ddd0c208
//# 4dce25ff9b06cde535d0f920a2db1bf3 62c23e596d11a4f5a6cf3948838a3aec
//# 4e15daf8500a6ef69ec4e3feb6b1d98e 610ac8b7ec3faf6ad760b7bad1db4ba3
//# 485e8a94dc250ae3fdb41ed15fb6a8e5 eba0fc3dd60bc8e30c5c4287e53805db
//# 059ae0648db2f64264ed5e39be2e20d8 2df566da8dd5998ccabdae053060ae6c
//# 7b4378e846d29f37ed7b4ea9ec5d82e7 961b7f25a9323851f681d582363aa5f8
//# 9937f5a67258bf63ad6f1a0b1d96dbd4 faddfcefc5266ba6611722395c906556
//# be52afe3f565636ad1b17d508b73d874 3eeb524be22b3dcbc2c7468d54119c74
//# 68449a13d8e3b95811a198f3491de3e7 fe942b330407abf82a4ed7c1b311663a
//# c69890f4157015853d91e923037c227a 33cdd5ec281ca3f79c44546b9d90ca00
//# f064c99e3dd97911d39fe9c5d0b23a22 9a234cb36186c4819e8b9c5927726632
//# 291d6a418211cc2962e20fe47feb3edf 330f2c603a9d48c0fcb5699dbfe58964
//# 25c5bac4aee82e57a85aaf4e2513e4f0 5796b07ba2ee47d80506f8d2c25e50fd
//# 14de71e6c418559302f939b0e1abd576 f279c4b2e0feb85c1f28ff18f58891ff
//# ef132eef2fa09346aee33c28eb130ff2 8f5b766953334113211996d20011a198
//# e3fc433f9f2541010ae17c1bf202580f 6047472fb36857fe843b19f5984009dd
//# c324044e847a4f4a0ab34f719595de37 252d6235365e9b84392b061085349d73
//# 203a4a13e96f5432ec0fd4a1ee65accd d5e3904df54c1da510b0ff20dcc0c77f
//# cb2c0e0eb605cb0504db87632cf3d8b4 dae6e705769d1de354270123cb11450e
//# fc60ac47683d7b8d0f811365565fd98c 4c8eb936bcab8d069fc33bd801b03ade
//# a2e1fbc5aa463d08ca19896d2bf59a07 1b851e6c239052172f296bfb5e724047
//# 90a2181014f3b94a4e97d117b4381303 68cc39dbb2d198065ae3986547926cd2
//# 162f40a29f0c3c8745c0f50fba3852e5 66d44575c29d39a03f0cda721984b6f4
//# 40591f355e12d439ff150aab7613499d bd49adabc8676eef023b15b65bfc5ca0
//# 6948109f23f350db82123535eb8a7433 bdabcb909271a6ecbcb58b936a88cd4e
//# 8f2e6ff5800175f113253d8fa9ca8885 c2f552e657dc603f252e1a8e308f76f0
//# be79e2fb8f5d5fbbe2e30ecadd220723 c8c0aea8078cdfcb3868263ff8f09400
//# 54da48781893a7e49ad5aff4af300cd8 04a6b6279ab3ff3afb64491c85194aab
//# 760d58a606654f9f4400e8b38591356f bf6425aca26dc85244259ff2b19c41b9
//# f96f3ca9ec1dde434da7d2d392b905dd f3d1f9af93d1af5950bd493f5aa731b4
//# 056df31bd267b6b90a079831aaf579be 0a39013137aac6d404f518cfd4684064
//# 7e78bfe706ca4cf5e9c5453e9f7cfd2b 8b4c8d169a44e55c88d4a9a7f9474241
//# e221af44860018ab0856972e194cd934

/// <https://www.rfc-editor.org/rfc/rfc9001#appendix-A.2>
pub const EXAMPLE_CLIENT_INITIAL_PROTECTED_PACKET: [u8; 1200] = hex!(
    "
   c000000001088394c8f03e5157080000 449e7b9aec34d1b1c98dd7689fb8ec11
   d242b123dc9bd8bab936b47d92ec356c 0bab7df5976d27cd449f63300099f399
   1c260ec4c60d17b31f8429157bb35a12 82a643a8d2262cad67500cadb8e7378c
   8eb7539ec4d4905fed1bee1fc8aafba1 7c750e2c7ace01e6005f80fcb7df6212
   30c83711b39343fa028cea7f7fb5ff89 eac2308249a02252155e2347b63d58c5
   457afd84d05dfffdb20392844ae81215 4682e9cf012f9021a6f0be17ddd0c208
   4dce25ff9b06cde535d0f920a2db1bf3 62c23e596d11a4f5a6cf3948838a3aec
   4e15daf8500a6ef69ec4e3feb6b1d98e 610ac8b7ec3faf6ad760b7bad1db4ba3
   485e8a94dc250ae3fdb41ed15fb6a8e5 eba0fc3dd60bc8e30c5c4287e53805db
   059ae0648db2f64264ed5e39be2e20d8 2df566da8dd5998ccabdae053060ae6c
   7b4378e846d29f37ed7b4ea9ec5d82e7 961b7f25a9323851f681d582363aa5f8
   9937f5a67258bf63ad6f1a0b1d96dbd4 faddfcefc5266ba6611722395c906556
   be52afe3f565636ad1b17d508b73d874 3eeb524be22b3dcbc2c7468d54119c74
   68449a13d8e3b95811a198f3491de3e7 fe942b330407abf82a4ed7c1b311663a
   c69890f4157015853d91e923037c227a 33cdd5ec281ca3f79c44546b9d90ca00
   f064c99e3dd97911d39fe9c5d0b23a22 9a234cb36186c4819e8b9c5927726632
   291d6a418211cc2962e20fe47feb3edf 330f2c603a9d48c0fcb5699dbfe58964
   25c5bac4aee82e57a85aaf4e2513e4f0 5796b07ba2ee47d80506f8d2c25e50fd
   14de71e6c418559302f939b0e1abd576 f279c4b2e0feb85c1f28ff18f58891ff
   ef132eef2fa09346aee33c28eb130ff2 8f5b766953334113211996d20011a198
   e3fc433f9f2541010ae17c1bf202580f 6047472fb36857fe843b19f5984009dd
   c324044e847a4f4a0ab34f719595de37 252d6235365e9b84392b061085349d73
   203a4a13e96f5432ec0fd4a1ee65accd d5e3904df54c1da510b0ff20dcc0c77f
   cb2c0e0eb605cb0504db87632cf3d8b4 dae6e705769d1de354270123cb11450e
   fc60ac47683d7b8d0f811365565fd98c 4c8eb936bcab8d069fc33bd801b03ade
   a2e1fbc5aa463d08ca19896d2bf59a07 1b851e6c239052172f296bfb5e724047
   90a2181014f3b94a4e97d117b4381303 68cc39dbb2d198065ae3986547926cd2
   162f40a29f0c3c8745c0f50fba3852e5 66d44575c29d39a03f0cda721984b6f4
   40591f355e12d439ff150aab7613499d bd49adabc8676eef023b15b65bfc5ca0
   6948109f23f350db82123535eb8a7433 bdabcb909271a6ecbcb58b936a88cd4e
   8f2e6ff5800175f113253d8fa9ca8885 c2f552e657dc603f252e1a8e308f76f0
   be79e2fb8f5d5fbbe2e30ecadd220723 c8c0aea8078cdfcb3868263ff8f09400
   54da48781893a7e49ad5aff4af300cd8 04a6b6279ab3ff3afb64491c85194aab
   760d58a606654f9f4400e8b38591356f bf6425aca26dc85244259ff2b19c41b9
   f96f3ca9ec1dde434da7d2d392b905dd f3d1f9af93d1af5950bd493f5aa731b4
   056df31bd267b6b90a079831aaf579be 0a39013137aac6d404f518cfd4684064
   7e78bfe706ca4cf5e9c5453e9f7cfd2b 8b4c8d169a44e55c88d4a9a7f9474241
   e221af44860018ab0856972e194cd934
    "
);

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.3
//# The server sends the following payload in response, including an ACK
//# frame, a CRYPTO frame, and no PADDING frames:
//#
//# 02000000000600405a020000560303ee fce7f7b37ba1d1632e96677825ddf739
//# 88cfc79825df566dc5430b9a045a1200 130100002e00330024001d00209d3c94
//# 0d89690b84d08a60993c144eca684d10 81287c834d5311bcf32bb9da1a002b00
//# 020304
//#

/// Example payload from <https://www.rfc-editor.org/rfc/rfc9001#appendix-A.3>
pub const EXAMPLE_SERVER_INITIAL_PAYLOAD: [u8; 99] = hex!(
    "
   02000000000600405a020000560303ee fce7f7b37ba1d1632e96677825ddf739
   88cfc79825df566dc5430b9a045a1200 130100002e00330024001d00209d3c94
   0d89690b84d08a60993c144eca684d10 81287c834d5311bcf32bb9da1a002b00
   020304
    "
);

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.3
//# The header from the server includes a new connection ID and a 2-byte
//# packet number encoding for a packet number of 1:
//#
//# c1000000010008f067a5502a4262b50040750001

pub const EXAMPLE_SERVER_INITIAL_HEADER: [u8; 20] =
    hex!("c1000000010008f067a5502a4262b50040750001");

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.3
//# As a result, after protection, the header protection sample is taken
//# starting from the third protected byte:
//#
//# sample = 2cd0991cd25b0aac406a5816b6394100
//# mask   = 2ec0d8356a
//# header = cf000000010008f067a5502a4262b5004075c0d9

#[test]
fn server_initial_protection_test() {
    let mask = hex!("2ec0d8356a");

    let unprotected_header = EXAMPLE_SERVER_INITIAL_HEADER;
    let protected_header = hex!("cf000000010008f067a5502a4262b5004075c0d9");
    let packet_tag = 0b01; // results in a 2 byte packet number

    header_protection_test_helper(mask, &unprotected_header, &protected_header, packet_tag);
}

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.3
//# The final protected packet is then:
//#
//# cf000000010008f067a5502a4262b500 4075c0d95a482cd0991cd25b0aac406a
//# 5816b6394100f37a1c69797554780bb3 8cc5a99f5ede4cf73c3ec2493a1839b3
//# dbcba3f6ea46c5b7684df3548e7ddeb9 c3bf9c73cc3f3bded74b562bfb19fb84
//# 022f8ef4cdd93795d77d06edbb7aaf2f 58891850abbdca3d20398c276456cbc4
//# 2158407dd074ee

/// Example protected packet from
/// <https://www.rfc-editor.org/rfc/rfc9001#appendix-A.3>
pub const EXAMPLE_SERVER_INITIAL_PROTECTED_PACKET: [u8; 135] = hex!(
    "
   cf000000010008f067a5502a4262b500 4075c0d95a482cd0991cd25b0aac406a
   5816b6394100f37a1c69797554780bb3 8cc5a99f5ede4cf73c3ec2493a1839b3
   dbcba3f6ea46c5b7684df3548e7ddeb9 c3bf9c73cc3f3bded74b562bfb19fb84
   022f8ef4cdd93795d77d06edbb7aaf2f 58891850abbdca3d20398c276456cbc4
   2158407dd074ee
    "
);

#[cfg(test)]
fn header_protection_test_helper(
    mask: crate::crypto::HeaderProtectionMask,
    unprotected_header: &[u8],
    protected_header: &[u8],
    packet_tag: u8,
) {
    use crate::{
        crypto::{
            apply_header_protection, remove_header_protection, EncryptedPayload, ProtectedPayload,
        },
        packet::number::PacketNumberSpace,
    };
    let space = PacketNumberSpace::Initial;

    let packet_number_len = space.new_packet_number_len(packet_tag);
    let header_len = protected_header.len() - packet_number_len.bytesize();

    let mut subject = protected_header.to_vec();

    remove_header_protection(space, mask, ProtectedPayload::new(header_len, &mut subject)).unwrap();

    assert_eq!(
        unprotected_header,
        &subject[..],
        "packet protection removal failed"
    );

    apply_header_protection(
        mask,
        EncryptedPayload::new(header_len, packet_number_len, &mut subject),
    );

    assert_eq!(
        protected_header,
        &subject[..],
        "packet protection application failed"
    );
}
