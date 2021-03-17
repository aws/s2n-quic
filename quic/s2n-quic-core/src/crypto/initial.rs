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

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.2
//# initial_salt = 0xafbfec289993d24c9e9786f19c6111e04390a899

pub const INITIAL_SALT: [u8; 20] = hex!("afbfec289993d24c9e9786f19c6111e04390a899");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.2
//# client_initial_secret = HKDF-Expand-Label(initial_secret,
//#                                           "client in", "",
//#                                           Hash.length)

pub const INITIAL_CLIENT_LABEL: [u8; 9] = *b"client in";

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.2
//# server_initial_secret = HKDF-Expand-Label(initial_secret,
//#                                           "server in", "",
//#                                           Hash.length)

pub const INITIAL_SERVER_LABEL: [u8; 9] = *b"server in";

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A
//# These packets use an 8-byte client-chosen Destination Connection ID
//# of 0x8394c8f03e515708.

pub const EXAMPLE_DCID: [u8; 8] = hex!("8394c8f03e515708");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.1
//# client_initial_secret
//#     = HKDF-Expand-Label(initial_secret, "client in", _, 32)
//#     = 0088119288f1d866733ceeed15ff9d50
//#       902cf82952eee27e9d4d4918ea371d87

pub const EXAMPLE_CLIENT_INITIAL_SECRET: [u8; 32] = hex!(
    "
    0088119288f1d866733ceeed15ff9d50
    902cf82952eee27e9d4d4918ea371d87
    "
);

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.1
//# server_initial_secret
//#     = HKDF-Expand-Label(initial_secret, "server in", _, 32)
//#     = 006f881359244dd9ad1acf85f595bad6
//#       7c13f9f5586f5e64e1acae1d9ea8f616

pub const EXAMPLE_SERVER_INITIAL_SECRET: [u8; 32] = hex!(
    "
    006f881359244dd9ad1acf85f595bad6
    7c13f9f5586f5e64e1acae1d9ea8f616
    "
);

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.2
//# The client sends an Initial packet.  The unprotected payload of this
//# packet contains the following CRYPTO frame, plus enough PADDING
//# frames to make an 1162 byte payload:
//#
//# 060040f1010000ed0303ebf8fa56f129 39b9584a3896472ec40bb863cfd3e868
//# 04fe3a47f06a2b69484c000004130113 02010000c000000010000e00000b6578
//# 616d706c652e636f6dff01000100000a 00080006001d00170018001000070005
//# 04616c706e0005000501000000000033 00260024001d00209370b2c9caa47fba
//# baf4559fedba753de171fa71f50f1ce1 5d43e994ec74d748002b000302030400
//# 0d0010000e0403050306030203080408 050806002d00020101001c00024001ff
//# a500320408ffffffffffffffff050480 00ffff07048000ffff08011001048000
//# 75300901100f088394c8f03e51570806 048000ffff

/// Example payload from https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.2
pub const EXAMPLE_CLIENT_INITIAL_PAYLOAD: [u8; 245] = hex!(
    "
   060040f1010000ed0303ebf8fa56f129 39b9584a3896472ec40bb863cfd3e868
   04fe3a47f06a2b69484c000004130113 02010000c000000010000e00000b6578
   616d706c652e636f6dff01000100000a 00080006001d00170018001000070005
   04616c706e0005000501000000000033 00260024001d00209370b2c9caa47fba
   baf4559fedba753de171fa71f50f1ce1 5d43e994ec74d748002b000302030400
   0d0010000e0403050306030203080408 050806002d00020101001c00024001ff
   a500320408ffffffffffffffff050480 00ffff07048000ffff08011001048000
   75300901100f088394c8f03e51570806 048000ffff
    "
);

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.2
//# The unprotected header includes the connection ID and a 4 byte packet
//# number encoding for a packet number of 2:
//#
//# c3ff000020088394c8f03e5157080000449e00000002

pub const EXAMPLE_CLIENT_INITIAL_HEADER: [u8; 22] =
    hex!("c3ff000020088394c8f03e5157080000449e00000002");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.2
//# Protecting the payload produces output that is sampled for header
//# protection.  Because the header uses a 4 byte packet number encoding,
//# the first 16 bytes of the protected payload is sampled, then applied
//# to the header:
//#
//# sample = fb66bc6a93032b50dd8973972d149421
//#
//# mask = AES-ECB(hp, sample)[0..4]
//#      = 1e9cdb9909
//#
//# header[0] ^= mask[0] & 0x0f
//#      = cd
//# header[18..21] ^= mask[1..4]
//#      = 9cdb990b
//# header = cdff000020088394c8f03e5157080000449e9cdb990b

#[test]
fn client_initial_protection_test() {
    let mask = hex!("1e9cdb9909");
    let unprotected_header = EXAMPLE_CLIENT_INITIAL_HEADER;
    let protected_header = hex!("cdff000020088394c8f03e5157080000449e9cdb990b");
    let packet_tag = 0b11; // results in 4 byte packet number

    header_protection_test_helper(mask, &unprotected_header, &protected_header, packet_tag);
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.2
//# The resulting protected packet is:
//#
//# cdff000020088394c8f03e5157080000 449e9cdb990bfb66bc6a93032b50dd89
//# 73972d149421874d3849e3708d71354e a33bcdc356f3ea6e2a1a1bd7c3d14003
//# 8d3e784d04c30a2cdb40c32523aba2da fe1c1bf3d27a6be38fe38ae033fbb071
//# 3c1c73661bb6639795b42b97f77068ea d51f11fbf9489af2501d09481e6c64d4
//# b8551cd3cea70d830ce2aeeec789ef55 1a7fbe36b3f7e1549a9f8d8e153b3fac
//# 3fb7b7812c9ed7c20b4be190ebd89956 26e7f0fc887925ec6f0606c5d36aa81b
//# ebb7aacdc4a31bb5f23d55faef5c5190 5783384f375a43235b5c742c78ab1bae
//# 0a188b75efbde6b3774ed61282f9670a 9dea19e1566103ce675ab4e21081fb58
//# 60340a1e88e4f10e39eae25cd685b109 29636d4f02e7fad2a5a458249f5c0298
//# a6d53acbe41a7fc83fa7cc01973f7a74 d1237a51974e097636b6203997f921d0
//# 7bc1940a6f2d0de9f5a11432946159ed 6cc21df65c4ddd1115f86427259a196c
//# 7148b25b6478b0dc7766e1c4d1b1f515 9f90eabc61636226244642ee148b464c
//# 9e619ee50a5e3ddc836227cad938987c 4ea3c1fa7c75bbf88d89e9ada642b2b8
//# 8fe8107b7ea375b1b64889a4e9e5c38a 1c896ce275a5658d250e2d76e1ed3a34
//# ce7e3a3f383d0c996d0bed106c2899ca 6fc263ef0455e74bb6ac1640ea7bfedc
//# 59f03fee0e1725ea150ff4d69a7660c5 542119c71de270ae7c3ecfd1af2c4ce5
//# 51986949cc34a66b3e216bfe18b347e6 c05fd050f85912db303a8f054ec23e38
//# f44d1c725ab641ae929fecc8e3cefa56 19df4231f5b4c009fa0c0bbc60bc75f7
//# 6d06ef154fc8577077d9d6a1d2bd9bf0 81dc783ece60111bea7da9e5a9748069
//# d078b2bef48de04cabe3755b197d52b3 2046949ecaa310274b4aac0d008b1948
//# c1082cdfe2083e386d4fd84c0ed0666d 3ee26c4515c4fee73433ac703b690a9f
//# 7bf278a77486ace44c489a0c7ac8dfe4 d1a58fb3a730b993ff0f0d61b4d89557
//# 831eb4c752ffd39c10f6b9f46d8db278 da624fd800e4af85548a294c1518893a
//# 8778c4f6d6d73c93df200960104e062b 388ea97dcf4016bced7f62b4f062cb6c
//# 04c20693d9a0e3b74ba8fe74cc012378 84f40d765ae56a51688d985cf0ceaef4
//# 3045ed8c3f0c33bced08537f6882613a cd3b08d665fce9dd8aa73171e2d3771a
//# 61dba2790e491d413d93d987e2745af2 9418e428be34941485c93447520ffe23
//# 1da2304d6a0fd5d07d08372202369661 59bef3cf904d722324dd852513df39ae
//# 030d8173908da6364786d3c1bfcb19ea 77a63b25f1e7fc661def480c5d00d444
//# 56269ebd84efd8e3a8b2c257eec76060 682848cbf5194bc99e49ee75e4d0d254
//# bad4bfd74970c30e44b65511d4ad0e6e c7398e08e01307eeeea14e46ccd87cf3
//# 6b285221254d8fc6a6765c524ded0085 dca5bd688ddf722e2c0faf9d0fb2ce7a
//# 0c3f2cee19ca0ffba461ca8dc5d2c817 8b0762cf67135558494d2a96f1a139f0
//# edb42d2af89a9c9122b07acbc29e5e72 2df8615c343702491098478a389c9872
//# a10b0c9875125e257c7bfdf27eef4060 bd3d00f4c14fd3e3496c38d3c5d1a566
//# 8c39350effbc2d16ca17be4ce29f02ed 969504dda2a8c6b9ff919e693ee79e09
//# 089316e7d1d89ec099db3b2b268725d8 88536a4b8bf9aee8fb43e82a4d919d48
//# b5a464ca5b62df3be35ee0d0a2ec68f3

/// Example protected packet from
/// https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.2
pub const EXAMPLE_CLIENT_INITIAL_PROTECTED_PACKET: [u8; 1200] = hex!(
    "
   cdff000020088394c8f03e5157080000 449e9cdb990bfb66bc6a93032b50dd89
   73972d149421874d3849e3708d71354e a33bcdc356f3ea6e2a1a1bd7c3d14003
   8d3e784d04c30a2cdb40c32523aba2da fe1c1bf3d27a6be38fe38ae033fbb071
   3c1c73661bb6639795b42b97f77068ea d51f11fbf9489af2501d09481e6c64d4
   b8551cd3cea70d830ce2aeeec789ef55 1a7fbe36b3f7e1549a9f8d8e153b3fac
   3fb7b7812c9ed7c20b4be190ebd89956 26e7f0fc887925ec6f0606c5d36aa81b
   ebb7aacdc4a31bb5f23d55faef5c5190 5783384f375a43235b5c742c78ab1bae
   0a188b75efbde6b3774ed61282f9670a 9dea19e1566103ce675ab4e21081fb58
   60340a1e88e4f10e39eae25cd685b109 29636d4f02e7fad2a5a458249f5c0298
   a6d53acbe41a7fc83fa7cc01973f7a74 d1237a51974e097636b6203997f921d0
   7bc1940a6f2d0de9f5a11432946159ed 6cc21df65c4ddd1115f86427259a196c
   7148b25b6478b0dc7766e1c4d1b1f515 9f90eabc61636226244642ee148b464c
   9e619ee50a5e3ddc836227cad938987c 4ea3c1fa7c75bbf88d89e9ada642b2b8
   8fe8107b7ea375b1b64889a4e9e5c38a 1c896ce275a5658d250e2d76e1ed3a34
   ce7e3a3f383d0c996d0bed106c2899ca 6fc263ef0455e74bb6ac1640ea7bfedc
   59f03fee0e1725ea150ff4d69a7660c5 542119c71de270ae7c3ecfd1af2c4ce5
   51986949cc34a66b3e216bfe18b347e6 c05fd050f85912db303a8f054ec23e38
   f44d1c725ab641ae929fecc8e3cefa56 19df4231f5b4c009fa0c0bbc60bc75f7
   6d06ef154fc8577077d9d6a1d2bd9bf0 81dc783ece60111bea7da9e5a9748069
   d078b2bef48de04cabe3755b197d52b3 2046949ecaa310274b4aac0d008b1948
   c1082cdfe2083e386d4fd84c0ed0666d 3ee26c4515c4fee73433ac703b690a9f
   7bf278a77486ace44c489a0c7ac8dfe4 d1a58fb3a730b993ff0f0d61b4d89557
   831eb4c752ffd39c10f6b9f46d8db278 da624fd800e4af85548a294c1518893a
   8778c4f6d6d73c93df200960104e062b 388ea97dcf4016bced7f62b4f062cb6c
   04c20693d9a0e3b74ba8fe74cc012378 84f40d765ae56a51688d985cf0ceaef4
   3045ed8c3f0c33bced08537f6882613a cd3b08d665fce9dd8aa73171e2d3771a
   61dba2790e491d413d93d987e2745af2 9418e428be34941485c93447520ffe23
   1da2304d6a0fd5d07d08372202369661 59bef3cf904d722324dd852513df39ae
   030d8173908da6364786d3c1bfcb19ea 77a63b25f1e7fc661def480c5d00d444
   56269ebd84efd8e3a8b2c257eec76060 682848cbf5194bc99e49ee75e4d0d254
   bad4bfd74970c30e44b65511d4ad0e6e c7398e08e01307eeeea14e46ccd87cf3
   6b285221254d8fc6a6765c524ded0085 dca5bd688ddf722e2c0faf9d0fb2ce7a
   0c3f2cee19ca0ffba461ca8dc5d2c817 8b0762cf67135558494d2a96f1a139f0
   edb42d2af89a9c9122b07acbc29e5e72 2df8615c343702491098478a389c9872
   a10b0c9875125e257c7bfdf27eef4060 bd3d00f4c14fd3e3496c38d3c5d1a566
   8c39350effbc2d16ca17be4ce29f02ed 969504dda2a8c6b9ff919e693ee79e09
   089316e7d1d89ec099db3b2b268725d8 88536a4b8bf9aee8fb43e82a4d919d48
   b5a464ca5b62df3be35ee0d0a2ec68f3
    "
);

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.3
//# The server sends the following payload in response, including an ACK
//# frame, a CRYPTO frame, and no PADDING frames:
//#
//# 02000000000600405a020000560303ee fce7f7b37ba1d1632e96677825ddf739
//# 88cfc79825df566dc5430b9a045a1200 130100002e00330024001d00209d3c94
//# 0d89690b84d08a60993c144eca684d10 81287c834d5311bcf32bb9da1a002b00
//# 020304

/// Example payload from https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.3
pub const EXAMPLE_SERVER_INITIAL_PAYLOAD: [u8; 99] = hex!(
    "
   02000000000600405a020000560303ee fce7f7b37ba1d1632e96677825ddf739
   88cfc79825df566dc5430b9a045a1200 130100002e00330024001d00209d3c94
   0d89690b84d08a60993c144eca684d10 81287c834d5311bcf32bb9da1a002b00
   020304
    "
);

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.3
//# The header from the server includes a new connection ID and a 2-byte
//# packet number encoding for a packet number of 1:
//#
//# c1ff0000200008f067a5502a4262b50040750001

pub const EXAMPLE_SERVER_INITIAL_HEADER: [u8; 20] =
    hex!("c1ff0000200008f067a5502a4262b50040750001");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.3
//# As a result, after protection, the header protection sample is taken
//# starting from the third protected octet:
//#
//# sample = 823a5d24534d906ce4c76782a2167e34
//# mask   = abaaf34fdc
//# header = c7ff0000200008f067a5502a4262b5004075fb12

#[test]
fn server_initial_protection_test() {
    // the mask in draft-32 is incorrect! this value was derived from running their samples script
    // https://github.com/quicwg/base-drafts/blob/master/protection-samples.js
    let mask = hex!("56fb1381e7");

    let unprotected_header = EXAMPLE_SERVER_INITIAL_HEADER;
    let protected_header = hex!("c7ff0000200008f067a5502a4262b5004075fb12");
    let packet_tag = 0b01; // results in a 2 byte packet number

    header_protection_test_helper(mask, &unprotected_header, &protected_header, packet_tag);
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.3
//# The final protected packet is then:
//#
//# c7ff0000200008f067a5502a4262b500 4075fb12ff07823a5d24534d906ce4c7
//# 6782a2167e3479c0f7f6395dc2c91676 302fe6d70bb7cbeb117b4ddb7d173498
//# 44fd61dae200b8338e1b932976b61d91 e64a02e9e0ee72e3a6f63aba4ceeeec5
//# be2f24f2d86027572943533846caa13e 6f163fb257473d0eda5047360fd4a47e
//# fd8142fafc0f76

/// Example protected packet from
/// https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.3
pub const EXAMPLE_SERVER_INITIAL_PROTECTED_PACKET: [u8; 135] = hex!(
    "
   c7ff0000200008f067a5502a4262b500 4075fb12ff07823a5d24534d906ce4c7
   6782a2167e3479c0f7f6395dc2c91676 302fe6d70bb7cbeb117b4ddb7d173498
   44fd61dae200b8338e1b932976b61d91 e64a02e9e0ee72e3a6f63aba4ceeeec5
   be2f24f2d86027572943533846caa13e 6f163fb257473d0eda5047360fd4a47e
   fd8142fafc0f76
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
