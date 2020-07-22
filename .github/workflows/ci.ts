const rustfmtSteps = [
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

const rustfmt = {
  "runs-on": "ubuntu-latest",
  needs: "env",
  steps: rustfmtSteps,
};

console.log(`::set-output name=rustfmt::${JSON.stringify(rustfmt)}`);
