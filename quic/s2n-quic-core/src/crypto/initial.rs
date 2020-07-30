use crate::crypto::{HeaderCrypto, Key};
use hex_literal::hex;

/// Types for which are able to perform initial cryptography.
///
/// This marker trait ensures only Initial-level keys
/// are used with Initial packets. Any key misuses are
/// caught by the type system.
pub trait InitialCrypto: Key + HeaderCrypto {
    fn new_server(connection_id: &[u8]) -> Self;
    fn new_client(connection_id: &[u8]) -> Self;
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#5.2
//# Initial packets are protected with a secret derived from the
//# Destination Connection ID field from the client's Initial packet.
//# Specifically:
//#
//# initial_salt = 0xafbfec289993d24c9e9786f19c6111e04390a899

pub const INITIAL_SALT: [u8; 20] = hex!("afbfec289993d24c9e9786f19c6111e04390a899");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#5.2
//# client_initial_secret = HKDF-Expand-Label(initial_secret,
//#                                           "client in", "",
//#                                           Hash.length)

pub const INITIAL_CLIENT_LABEL: [u8; 9] = *b"client in";

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#5.2
//# server_initial_secret = HKDF-Expand-Label(initial_secret,
//#                                           "server in", "",
//#                                           Hash.length)

pub const INITIAL_SERVER_LABEL: [u8; 9] = *b"server in";

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A
//# These packets
//# use an 8-byte client-chosen Destination Connection ID of
//# 0x8394c8f03e515708.

pub const EXAMPLE_DCID: [u8; 8] = hex!("8394c8f03e515708");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.1
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

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.1
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

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.2
//# The client sends an Initial packet.  The unprotected payload of this
//# packet contains the following CRYPTO frame, plus enough PADDING
//# frames to make an 1162 byte payload:
//#
//# 060040c4010000c003036660261ff947 cea49cce6cfad687f457cf1b14531ba1
//# 4131a0e8f309a1d0b9c4000006130113 031302010000910000000b0009000006
//# 736572766572ff01000100000a001400 12001d00170018001901000101010201
//# 03010400230000003300260024001d00 204cfdfcd178b784bf328cae793b136f
//# 2aedce005ff183d7bb14952072366470 37002b0003020304000d0020001e0403
//# 05030603020308040805080604010501 060102010402050206020202002d0002
//# 0101001c00024001

/// Example payload from https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.2
pub const EXAMPLE_CLIENT_INITIAL_PAYLOAD: [u8; 200] = hex!(
    "
    060040c4010000c003036660261ff947 cea49cce6cfad687f457cf1b14531ba1
    4131a0e8f309a1d0b9c4000006130113 031302010000910000000b0009000006
    736572766572ff01000100000a001400 12001d00170018001901000101010201
    03010400230000003300260024001d00 204cfdfcd178b784bf328cae793b136f
    2aedce005ff183d7bb14952072366470 37002b0003020304000d0020001e0403
    05030603020308040805080604010501 060102010402050206020202002d0002
    0101001c00024001
    "
);

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.2
//# The unprotected header includes the connection ID and a 4 byte packet
//# number encoding for a packet number of 2:
//#
//# c3ff00001d088394c8f03e5157080000449e00000002

pub const EXAMPLE_CLIENT_INITIAL_HEADER: [u8; 22] =
    hex!("c3ff00001d088394c8f03e5157080000449e00000002");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.2
//# Protecting the payload produces output that is sampled for header
//# protection.  Because the header uses a 4 byte packet number encoding,
//# the first 16 bytes of the protected payload is sampled, then applied
//# to the header:
//#
//# sample = fb66bc5f93032b7ddd89fe0ff15d9c4f
//#
//# mask = AES-ECB(hp, sample)[0..4]
//#      = d64a952459
//#
//# header[0] ^= mask[0] & 0x0f
//#      = c5
//# header[18..21] ^= mask[1..4]
//#      = 4a95245b
//# header = c5ff00001d088394c8f03e5157080000449e4a95245b

#[test]
fn client_initial_protection_test() {
    let mask = hex!("d64a952459");
    let unprotected_header = EXAMPLE_CLIENT_INITIAL_HEADER;
    let protected_header = hex!("c5ff00001d088394c8f03e5157080000449e4a95245b");
    let packet_tag = 0b11; // results in 4 byte packet number

    header_protection_test_helper(mask, &unprotected_header, &protected_header, packet_tag);
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.2
//# The resulting protected packet is:
//#
//# c5ff00001d088394c8f03e5157080000 449e4a95245bfb66bc5f93032b7ddd89
//# fe0ff15d9c4f7050fccdb71c1cd80512 d4431643a53aafa1b0b518b44968b18b
//# 8d3e7a4d04c30b3ed9410325b2abb2da fb1c12f8b70479eb8df98abcaf95dd8f
//# 3d1c78660fbc719f88b23c8aef6771f3 d50e10fdfb4c9d92386d44481b6c52d5
//# 9e5538d3d3942de9f13a7f8b702dc317 24180da9df22714d01003fc5e3d165c9
//# 50e630b8540fbd81c9df0ee63f949970 26c4f2e1887a2def79050ac2d86ba318
//# e0b3adc4c5aa18bcf63c7cf8e85f5692 49813a2236a7e72269447cd1c755e451
//# f5e77470eb3de64c8849d29282069802 9cfa18e5d66176fe6e5ba4ed18026f90
//# 900a5b4980e2f58e39151d5cd685b109 29636d4f02e7fad2a5a458249f5c0298
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
//# 43b1ca70a2d8d3f725ead1391377dcc0

/// Example protected packet from
/// https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.2
pub const EXAMPLE_CLIENT_INITIAL_PROTECTED_PACKET: [u8; 1200] = hex!(
    "
    c5ff00001d088394c8f03e5157080000 449e4a95245bfb66bc5f93032b7ddd89
    fe0ff15d9c4f7050fccdb71c1cd80512 d4431643a53aafa1b0b518b44968b18b
    8d3e7a4d04c30b3ed9410325b2abb2da fb1c12f8b70479eb8df98abcaf95dd8f
    3d1c78660fbc719f88b23c8aef6771f3 d50e10fdfb4c9d92386d44481b6c52d5
    9e5538d3d3942de9f13a7f8b702dc317 24180da9df22714d01003fc5e3d165c9
    50e630b8540fbd81c9df0ee63f949970 26c4f2e1887a2def79050ac2d86ba318
    e0b3adc4c5aa18bcf63c7cf8e85f5692 49813a2236a7e72269447cd1c755e451
    f5e77470eb3de64c8849d29282069802 9cfa18e5d66176fe6e5ba4ed18026f90
    900a5b4980e2f58e39151d5cd685b109 29636d4f02e7fad2a5a458249f5c0298
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
    43b1ca70a2d8d3f725ead1391377dcc0
    "
);

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.3
//# The server sends the following payload in response, including an ACK
//# frame, a CRYPTO frame, and no PADDING frames:
//#
//# 0d0000000018410a020000560303eefc e7f7b37ba1d1632e96677825ddf73988
//# cfc79825df566dc5430b9a045a120013 0100002e00330024001d00209d3c940d
//# 89690b84d08a60993c144eca684d1081 287c834d5311bcf32bb9da1a002b0002
//# 0304

/// Example payload from https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.3
pub const EXAMPLE_SERVER_INITIAL_PAYLOAD: [u8; 98] = hex!(
    "
    0d0000000018410a020000560303eefc e7f7b37ba1d1632e96677825ddf73988
    cfc79825df566dc5430b9a045a120013 0100002e00330024001d00209d3c940d
    89690b84d08a60993c144eca684d1081 287c834d5311bcf32bb9da1a002b0002
    0304
    "
);

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.3
//# The header from the server includes a new connection ID and a 2-byte
//# packet number encoding for a packet number of 1:
//#
//# c1ff00001d0008f067a5502a4262b50040740001

pub const EXAMPLE_SERVER_INITIAL_HEADER: [u8; 20] =
    hex!("c1ff00001d0008f067a5502a4262b50040740001");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.3
//# As a result, after protection, the header protection sample is taken
//# starting from the third protected octet:
//#
//# sample = 823a5d3a1207c86ee49132824f046524
//# mask   = abaaf34fdc
//# header = caff00001d0008f067a5502a4262b5004074aaf2

#[test]
fn server_initial_protection_test() {
    let mask = hex!("abaaf34fdc");
    let unprotected_header = EXAMPLE_SERVER_INITIAL_HEADER;
    let protected_header = hex!("caff00001d0008f067a5502a4262b5004074aaf2");
    let packet_tag = 0b01; // results in a 2 byte packet number

    header_protection_test_helper(mask, &unprotected_header, &protected_header, packet_tag);
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.3
//# The final protected packet is then:
//#
//# caff00001d0008f067a5502a4262b500 4074aaf2f007823a5d3a1207c86ee491
//# 32824f0465243d082d868b107a38092b c80528664cbf9456ebf27673fb5fa506
//# 1ab573c9f001b81da028a00d52ab00b1 5bebaa70640e106cf2acd043e9c6b441
//# 1c0a79637134d8993701fe779e58c2fe 753d14b0564021565ea92e57bc6faf56
//# dfc7a40870e6

/// Example protected packet from
/// https://tools.ietf.org/id/draft-ietf-quic-tls-29.txt#A.3
pub const EXAMPLE_SERVER_INITIAL_PROTECTED_PACKET: [u8; 134] = hex!(
    "
    caff00001d0008f067a5502a4262b500 4074aaf2f007823a5d3a1207c86ee491
    32824f0465243d082d868b107a38092b c80528664cbf9456ebf27673fb5fa506
    1ab573c9f001b81da028a00d52ab00b1 5bebaa70640e106cf2acd043e9c6b441
    1c0a79637134d8993701fe779e58c2fe 753d14b0564021565ea92e57bc6faf56
    dfc7a40870e6
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
    )
    .unwrap();

    assert_eq!(
        protected_header,
        &subject[..],
        "packet protection application failed"
    );
}
