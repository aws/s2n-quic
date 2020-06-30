use bolero::fuzz;
use s2n_codec::assert_codec_round_trip_bytes_mut;
use s2n_quic_core::frame::FrameRef;

fn main() {
    fuzz!().for_each(|input| {
        let mut input = input.to_vec();
        assert_codec_round_trip_bytes_mut!(FrameRef, &mut input);
    });
}
