// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

pub use super::common::*;
pub type Instruction = super::Instruction<Ebpf>;
pub type Program<'a> = super::Program<'a, Ebpf>;

pub struct Ebpf;

impl super::instruction::Dialect for Ebpf {
    const MAX_INSTRUCTIONS: usize = u16::MAX as _;
    const SOCKOPT: libc::c_int = libc::SO_ATTACH_REUSEPORT_EBPF as _;

    fn debug(i: &Instruction, f: &mut fmt::Formatter) -> fmt::Result {
        let code = i.code;
        let k = i.k;
        let jt = i.jt;
        let jf = i.jf;
        let alt = f.alternate();

        let mut f = f.debug_struct("Instruction");
        let class = Class::decode(code);

        if alt {
            f.field("code", &code).field("jt", &jt).field("jf", &jf);
        }

        f.field("class", &class);

        match class {
            Class::ALU => {
                f.field("op", &Alu::decode(code));
            }
            Class::JMP | Class::JMP32 => {
                f.field("op", &Jump::decode(code));
            }
            Class::LD | Class::LDX => {
                f.field("size", &Size::decode(code))
                    .field("mode", &Mode::decode(code));
            }
            // TODO other classes
            _ => {}
        }

        if jt > 0 {
            f.field("jt", &jt);
        }

        if jf > 0 {
            f.field("jf", &jf);
        }

        f.field("k", &k).finish()
    }

    fn display(i: &Instruction, f: &mut fmt::Formatter, line: Option<usize>) -> fmt::Result {
        let code = i.code;
        let k = i.k;
        let jt = i.jt;
        let jf = i.jf;

        if let Some(line) = line {
            write!(f, "l{line:<4}: ")?;
        }

        let class = Class::decode(code);

        match class {
            Class::LD | Class::LDX => {
                let size = Size::decode(code).suffix();
                let mode = Mode::decode(code);

                match mode {
                    Mode::IMM => return write!(f, "{class}{size} #{k}"),
                    Mode::ABS => {
                        let prefix = if k == 0 { "" } else { "0x" };
                        return if let Some(info) = super::ancillary::lookup(k) {
                            write!(
                                f,
                                "{class}{size} {} ; [{prefix}{k:x}] // {}",
                                info.extension, info.capi
                            )
                        } else {
                            write!(f, "{class}{size} [{prefix}{k:x}]")
                        };
                    }
                    _ => {}
                }
            }
            Class::ALU => {
                let op = Alu::decode(code);
                let source = Source::decode(code);

                return match source {
                    Source::K => write!(f, "{op} #{k}"),
                    Source::X => write!(f, "{op} x"),
                };
            }
            Class::JMP | Class::JMP32 => {
                let op = Jump::decode(code);
                let source = Source::decode(code);

                match source {
                    Source::K => write!(f, "{op} #{k}")?,
                    Source::X => write!(f, "{op} x")?,
                }

                if let Some(line) = line {
                    let line = line + 1;
                    let jt = line + jt as usize;
                    let jf = line + jf as usize;
                    write!(f, ",l{jt},l{jf}")?
                } else {
                    write!(f, ",{jt},{jf}")?
                }

                return Ok(());
            }
            _ => {}
        }

        write!(f, "<unknown instruction {i:?}>")
    }
}

// https://www.kernel.org/doc/html/next/bpf/instruction-set.html#instruction-classes
define!(
    #[mask(0x07)]
    pub enum Class {
        LD = 0x00,
        LDX = 0x01,
        ST = 0x02,
        STX = 0x03,
        ALU = 0x04,
        JMP = 0x05,
        JMP32 = 0x06,
        ALU64 = 0x07,
    }
);

// https://www.kernel.org/doc/html/next/bpf/instruction-set.html#load-and-store-instructions
define!(
    #[mask(0x18)]
    pub enum Size {
        // word (4 bytes)
        W = 0x00,
        // half word (2 bytes)
        H = 0x08,
        // byte
        B = 0x10,
        // double word (8 bytes)
        DW = 0x18,
    }
);

impl Size {
    #[inline]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::W => "",
            Self::H => "H",
            Self::B => "B",
            Self::DW => "DW",
        }
    }
}

// https://www.kernel.org/doc/html/next/bpf/instruction-set.html#load-and-store-instructions
define!(
    #[mask(0xe0)]
    pub enum Mode {
        IMM = 0x00,
        ABS = 0x20,
        IND = 0x40,
        MEM = 0x60,
        ATOMIC = 0xc0,
    }
);

// https://www.kernel.org/doc/html/next/bpf/instruction-set.html#arithmetic-instructions
define!(
    #[mask(0xf0)]
    pub enum Alu {
        ADD = 0x00,
        SUB = 0x10,
        MUL = 0x20,
        DIV = 0x30,
        OR = 0x40,
        AND = 0x50,
        LSH = 0x60,
        RSH = 0x70,
        NEG = 0x80,
        MOD = 0x90,
        XOR = 0xa0,
        MOV = 0xb0,
        ARSH = 0xc0,
        END = 0xd0,
    }
);

// https://www.kernel.org/doc/html/next/bpf/instruction-set.html#jump-instructions
define!(
    #[mask(0xf0)]
    pub enum Jump {
        JA = 0x00,
        JEQ = 0x10,
        JGT = 0x20,
        JGE = 0x30,
        JSET = 0x40,
        JNE = 0x50,
        JSGET = 0x60,
        JSGE = 0x70,
        CALL = 0x80,
        EXIT = 0x90,
        JLT = 0xa0,
        JLE = 0xb0,
        JSLT = 0xc0,
        JSLE = 0xd0,
    }
);

// https://www.kernel.org/doc/html/next/bpf/instruction-set.html#byte-swap-instructions
define!(
    #[mask(0x0f)]
    pub enum Swap {
        TO_LE = 0x00,
        TO_BE = 0x08,
    }
);

impl_ops!();

pub const fn len() -> K {
    // still need to figure out what the API is for eBPF - cBPF has a dedicated Mode
    todo!()
}
