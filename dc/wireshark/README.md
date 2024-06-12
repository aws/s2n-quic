# dcQUIC Wireshark integration

This directory contains a Rust plugin for Wireshark, which supports dissecting
dcQUIC Datagram, Stream, Control, and Secret Control packets over UDP, and
Stream packets over TCP. (This is currently full support for what we send in
current versions of dcQUIC).

The plugin supports heuristic dissection, and will incrementally mark/record
fields in Wireshark even if the full packet does not parse as we expect. The
plugin does not currently support making use of any secret material to decrypt
payloads or verify authentication tags.

## Usage

The plugin is built against Wireshark version 4.2.5 headers. It's likely that a
new set of bindgen bindings are needed for other versions, and Wireshark will
refuse to load the plugin outside of the 4.2.x series (without code changes to
increment the supported minor version).

To install the plugin for the current machine, use the following command:

```
cargo xtask install
```

Once this is done, Wireshark should load the plugin successfully on startup.
You can check (even without a pcap) by (a) not seeing an error message and (b)
typing `dcquic` into the search bar, which should get auto-completed and
highlighted green as a valid search.

You can also use the plugin from the command line via `tshark`, for example:

```
tshark -r stream-request-response.pcap -O dcquic 'dcquic && not tcp'
```

## Contributing changes

If you need access to more Wireshark APIs that currently don't have bindings in
`src/wireshark_sys`, you can re-generate that file with
`./generate-bindings.sh`.

https://www.wireshark.org/docs/wsdg_html/#ChapterDissection is a good starting
point for understanding the basics of the Wireshark interface.

The tests are runnable without a Wireshark installation and are fairly good at
catching bugs unrelated to the specifics of Wireshark FFI (e.g., parser bugs
should be caught). We rely primarily on fuzz-style testing, both of valid
packets (to test fields are properly decoded) and of random packets (to ensure
lack of panics).

### Why a Rust plugin?

Wireshark supports Lua plugins, but they are comparatively much slower. In our
testing, a native plugin is 3.3x faster at performing the same body of work as
a Lua plugin. This cost adds up quickly, especially as we expect to frequently
work with fairly coarse packet captures that may contain millions of packets.

A Rust plugin also allows for direct interop with our existing code, both for
help in parsing (e.g., VarInt decoding) and in testing. These are obviously
possible to integrate into Lua, but would take extra dependencies and work.
