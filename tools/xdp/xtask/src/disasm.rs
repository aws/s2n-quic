// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::build_ebpf;
use anyhow::Context;
use std::io;

type File<'a> = elf::ElfBytes<'a, elf::endian::AnyEndian>;

pub fn run() -> Result<(), anyhow::Error> {
    build_ebpf::run().context("Error while building eBPF program")?;

    piped("bpfel", std::io::stdout())?;

    Ok(())
}

pub fn piped<O: io::Write>(input: &str, mut out: O) -> Result<(), anyhow::Error> {
    let prog = std::fs::read(format!("s2n-quic-xdp/src/bpf/s2n-quic-xdp-{input}.ebpf"))?;
    let file = File::minimal_parse(&prog)?;

    writeln!(out, "===== {input} =====")?;
    dump(&file, "xdp/s2n_quic_xdp", &mut out)?;
    dump(&file, ".text", &mut out)?;

    Ok(())
}

fn dump<O: io::Write>(file: &File, section: &str, mut out: O) -> Result<(), anyhow::Error> {
    if let Some(header) = file.section_header_by_name(section)? {
        if let Ok((data, _compression)) = file.section_data(&header) {
            writeln!(out, "----- {section} -----")?;
            for insn in rbpf::disassembler::to_insn_vec(data) {
                writeln!(out, "  {}", insn.desc)?;
            }
        }
    }

    Ok(())
}
