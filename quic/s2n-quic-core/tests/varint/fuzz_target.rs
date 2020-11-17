use bolero::check;
use s2n_codec::assert_codec_round_trip_bytes;
use s2n_quic_core::varint::VarInt;

fn main() {
    check!().for_each(|input| {
        for value in assert_codec_round_trip_bytes!(VarInt, input) {
            let _ = value.checked_add(value);
            let _ = value.checked_sub(value);
            let _ = value.checked_mul(value);
            let _ = value.checked_div(value);
            let _ = value.saturating_add(value);
            let _ = value.saturating_sub(value);
            let _ = value.saturating_mul(value);
        }
    });
}
