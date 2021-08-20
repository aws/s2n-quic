# s2n-quic-events

This crate generates all of the boilerplate for event types and traits.

## Features

* A single rust file is generated for all of the event handling code.
* The generator script is executed at commit time, which provides minimal compilation overhead.
* The output can be easily read and audited.
* IDEs can easily read the generated code and provide autocompletion on events.

## How does it work?

* The crate takes in any rust files in the `events` directory and parses them with [`syn`](https://crates.io/crates/syn).
* Each item is scanned for attributes to identify how it should hook into the event system.
* After all the files are scanned, each item returns a [proc-macro2](https://crates.io/crates/proc-macro2) `TokenStream` for various components of the output (e.g. the `Subscriber` trait, the `Publisher` traits, `testing` modules, etc).
* The `TokenStream` is combined into a single file and written to disk (`s2n-quic-core/src/event/generated.rs`).
* `rustfmt` is applied to the generated output to make it a little easier to read.

## FAQ

### Why not be explicit and write it by hand?

Defining new events should be as straightforward as possible. Ideally, it is as simple as defining a new struct or enum. However, there are several things the event must do in order to be usable:

* The `Subscriber` trait needs a `on_{event_name}` callback
* The `Publisher` traits needs a `on_{event_name}` callback
* The `Event` trait needs to be implemented.
* The `#[non_exhaustive]` attribute needs to be applied to everything to ensure applications are not broken
when new fields or variants are added.
* A builder type needs to also be generated so crates outside of `s2n-quic-core` can construct events. Constructing the type directly would be impossible with the `#[non_exhaustive]` attribute.
* The `impl<A, B> Subscriber for (A, B) { .. }` needs to be updated to forward the event on to the child-subscribers.
* The built-in subscribers (tracing, serde, etc) need to log the new event with the appropriate level.
* The test subscribers need to add a new counter for the event.

As such, we prefer a little build-time magic over easy-to-miss configuration.

### Why not a proc macro?

[Procedural macros](https://doc.rust-lang.org/reference/procedural-macros.html) make it easy to hook into the rust compilation process for any items and modify or generate new tokens. This has a few limitations for our use case:

* Procedural macros can only be applied to a specific item (i.e. struct, enum, module, etc.) Because events span multiple items, this would require wrapping the whole definition in a single macro call.
* Procedural macros are executed on every build of the crate. Since events won't change from build-to-build, it's hard to justify the overhead on consumers of the library.

### Why not a build script?

[Build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html) are another way to execute code at compile time. In this case they run before the crate is compiled. However, one of the same downsides of procedural macros applies here as well: consumers of the library will need to compile the build script and execute it every time the crate is built. For something that doesn't change often or doesn't depend on the environment in which it's being compiled, it doesn't seem worth it.
