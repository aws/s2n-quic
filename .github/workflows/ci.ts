const rustfmt = [
  {
    uses: "actions/checkout@v2"
  },
  {
    uses: "actions/toolchain@v1",
    id: "toolchain",
    with: {
      toolchain: "nightly-2020-07-11",
      profile: "minimal",
      override: true,
      components: "rustfmt",
    },
  },
  {
    name: "Run cargo fmt",
    uses: "actions-rs/cargo@v1",
    with: {
      command: "fmt",
      args: "--all -- --check",
    },
  },
];

console.log(`::set-output name=rustfmt::${JSON.stringify(rustfmt)}`);
