# s2n-quic-crypto

This crate contains QUIC-optimized versions of cryptographic AEAD routines for high efficiency and performance. As such, **it is not meant to be for general use** outside of `s2n-quic`. YOU HAVE BEEN WARNED!

## Navigating the code

The code in this crate is defined in several layers of abstraction, which allow the upper layers to define algorithms in a very high level with very little `unsafe` code. Starting from the lowest level going up, the crate is composed of several modules:

### arch

Architecture-specific intrinsics enable Rust to execute special CPU instructions optimized for cryptography. This module selects the target architecture and exports the available intrinsics. However, this doesn't mean that the final CPU will actually support the instructions and executing the code will result in an `Illegal instruction` error. This means we must probe for instruction support at runtime to make it easy for applications to get the most optimized version of the code. In Rust/x86 this is accomplished with the [`is_x86_feature_detected!`](https://doc.rust-lang.org/std/macro.is_x86_feature_detected.html) macro and the [`target_feature`](https://rust-lang.github.io/rfcs/2045-target-feature.html) attribute.

### block

Blocks define the unit of operation for block ciphers. In the case of AES, GHash, and AES-GCM this is a 128-bit value. Blocks can be operated on in "batches", which are arrays of blocks. This concept enables CPUs to look ahead of the program counter and perform computation in parallel. The batch size for AES-GCM in [AWS-LC](https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/modes/asm/aesni-gcm-x86_64.pl#L494) is `6`. After benchmarking several batch sizes in this code base (`4`, `6`, and `8`), this library has also selected `6` as a default.

### aes

This module contains AES implementations for each of the supported platforms. Both AES-128 and AES-256 are supported. Each implementation is generic over the `Encrypt` and `Decrypt` traits, making it easy to write generic code over the various key sizes. The AES traits also allow for interleaving instructions between rounds, which enables CPUs to perform multiple types of computation in parallel. This feature is used extensively in AES-GCM, as AES and GHash operations are performed in lockstep.

The AES implementations for x86 are a direct port of the [AWS-LC](https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl) code, as that implementation has been heavily optimized over the years. Since the `aes` instruction set performs most of the heavy lifting, there isn't really any further optimization that can be done.

### ghash

This module contains GHash implementations for each of the supported platforms. Each implementation is generic over the `GHash` trait, allowing usage to be decoupled from the implementation. This enables us to experiment with various optimizations in the GHash implementation.

On x86, there are 3 implementations of the GHash algorithm: `std`, `pre_h`, and `pre_hr`. The `std` implementation is the same you would find in [AWS-LC](https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/modes/asm/ghash-x86_64.pl). The `pre_h` and `pre_hr`, however, work quite a bit differently. Instead of calling `gf_mul` for each block update, all of the powers of `H`, up to the maximum input size, are precomputed at key time and only at the end of the digest is a reduction performed. This allows applications to make tradeoffs between memory and CPU efficiency. `pre_hr` takes it a bit further and precomputes the `r` value as well, which doubles the required memory. However, after benchmarking the two options, this seems to make very little difference, if any.

The precomputed modes allow for statically and dynamically defined sizes. The dynamic mode enables applications to "upgrade" the efficiency of a key after deciding it's going to be worth the memory footprint.

### aesgcm

This module aims to provide a generic, platform-independent implementation of the AES-GCM mode. This means it uses all of the previously-defined traits to construct its implementation. It's also generic over the batch size and can be defined on type instantiation.

In theory, this means that adding platform support only requires implementing the AES and GHash traits. In practice, it may not hold true as there only exists a `x86` implementation.

### testing

This module contains all of the support functionality for testing implementations. Since it isn't entirely known if an implementation will be supported by the CPU until runtime, each module has a `implementations` function that returns all of the supported implementations by the runtime. This allows the caller to iterate over all of the implementations of a particular algorithm, and perform operations and make assertions.

Each module has a `test_vector` test, which uses well-known inputs and asserts that the outputs match expectations.

Each module also has a `differential_test` test, which uses [`bolero`](https://camshaft.github.io/bolero/) to generate keys, payloads, nonces, etc. and compares outputs to several well-known implementations (currently [ring](https://github.com/briansmith/ring) and [RustCrypto](https://github.com/RustCrypto)). It also asserts that decrypting the payload results in the original plaintext. This allows us to quickly identify/prevent any differences in functionality.

There are also [criterion](https://crates.io/crates/criterion) benchmarks for each of the implementations. This provides a report of each of the outcomes and ensures performance is maintained across commits.