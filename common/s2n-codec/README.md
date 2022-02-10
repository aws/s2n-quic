# quic-codec

Utilities for decoding and encoding values in a safe and performance-oriented
way.

This is an internal crate used by [s2n-quic](https://github.com/aws/s2n-quic). The API is not currently stable and should not be used directly.

## Decoder

Consider the following code:

```rust
fn decode_u8(buffer: &[u8]) -> (u8, &[u8]) {
    let value = buffer[0];
    (value, buffer[1..])
}

decode_u8(&[1, 2, 3]); // => (1, &[2, 3])
decode_u8(&[4]); // => (4, &[])
```

While this is safe as far as Rust is concerned, this method will panic on
missing input:

```rust
decode_u8(&[]) // thread 'main' panicked at 'index out of bounds: the len is 0 but the index is 0'
```

These kind of issues can be hard to detect and can have a large impact on
environments like servers where untrusted data is being passed. An attacker
could potentially craft a payload that will crash the server.

One possible way to mitigate these issues is to perform a check:

```rust
fn decode_u8(buffer: &[u8]) -> Result<(u8, &[u8]), Error> {
    if buffer.len() < 1 {
        return Err(Error::OutOfBounds);
    }

    let value = buffer[0];
    Ok((value, buffer[1..]))
}

decode_u8(&[1, 2, 3]); // => Ok((1, &[2, 3]))
decode_u8(&[4]); // => Ok((4, &[]))
decode_u8(&[]); // => Err(Error::OutOfBounds)
```

This solution works for this particular case but is error-prone, as it requires
each access to the slice to assert its set of preconditions. Special care
especially needs to be taken when the length of a decoded value depends on a
previously decoded, untrusted input:

```rust
fn decode_slice(buffer: &[u8]) -> Result<(&[u8], &[u8]), Error> {
    if buffer.len() < 1 {
        return Err(Error::OutOfBounds);
    }

    let len = buffer[0] as usize;

    if buffer.len() < len {
        return Err(Error::OutOfBounds);
    }

    let value = buffer[1..len];
    Ok((value, buffer[len..]))
}
```

`quic-codec` instead provides an interface to a slice that is guaranteed not to
panic. It accomplishes this by forcing checks to occur and precondition
violations to be handled.

```rust
fn decode_u8(buffer: DecoderBuffer) -> DecoderResult<u8> {
    let (value, buffer) = buffer.decode::<u8>()?;
    Ok((value, buffer))
}
```

Another major advantage is gained through type-inferred decoding. The
`DecoderBuffer::decode` function can be extended to support any type, given it
implements the `DecoderValue` trait. Consider the following example where the
same `decode` function call is used to parse `u32`, `u8`, and `Date` itself:

```rust
struct Date {
    year: u32,
    month: u8,
    day: u8,
}

impl<'a> DecoderValue<'a> for Date {
    fn decode(buffer: DecoderBuffer<'a>) -> DecoderResult<'a, Self> {
        let (year, buffer) = buffer.decode()?;
        let (month, buffer) = buffer.decode()?;
        let (day, buffer) = buffer.decode()?;
        let date = Self { year, month, day };
        Ok((date, buffer))
    }
}

fn decode_two_dates(buffer: DecoderBuffer) -> DecoderResult<(Date, Date)> {
    let (first, buffer) = buffer.decode()?;
    let (second, buffer) = buffer.decode()?;
    Ok(((first, second), buffer))
}
```

## Encoder

The EncoderBuffer is the counterpart to DecoderBuffer. It writes any value that
implements the `EncoderValue` to a pre-allocated mutable slice. Each type gives
hints for the final the encoding size to ensure a single allocation when
encoding a value.
