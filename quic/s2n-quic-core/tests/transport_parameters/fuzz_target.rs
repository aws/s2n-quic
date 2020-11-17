use bolero::check;
use s2n_codec::assert_codec_round_trip_bytes;
use s2n_quic_core::transport::parameters::{ClientTransportParameters, ServerTransportParameters};

fn main() {
    check!().for_each(|input| {
        if input.is_empty() {
            return;
        }

        if input[0] > core::u8::MAX / 2 {
            assert_codec_round_trip_bytes!(ClientTransportParameters, input[1..]);
        } else {
            assert_codec_round_trip_bytes!(ServerTransportParameters, input[1..]);
        }
    });
}
