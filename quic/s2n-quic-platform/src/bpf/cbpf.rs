// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

#[cfg(all(test, not(miri)))]
mod tests;

pub use super::common::*;
pub type Instruction = super::Instruction<Cbpf>;
pub type Program<'a> = super::Program<'a, Cbpf>;

#[derive(Clone, Copy, Debug, Default)]
pub struct Cbpf;

impl super::instruction::Dialect for Cbpf {
    // https://github.com/torvalds/linux/blob/b947cc5bf6d793101135265352e205aeb30b54f0/include/uapi/linux/bpf_common.h#L54
    const MAX_INSTRUCTIONS: usize = 4096;
    const SOCKOPT: libc::c_int = libc::SO_ATTACH_REUSEPORT_CBPF as _;

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
            Class::JMP => {
                f.field("op", &Jump::decode(code));
            }
            Class::LD | Class::LDX | Class::RET => {
                f.field("size", &Size::decode(code))
                    .field("mode", &Mode::decode(code));
            }
            _ => {}
        }

        if jt > 0 {
            f.field("jt", &jt);
        }

        if jf > 0 {
            f.field("jf", &jf);
        }

        let prefix = if k == 0 { "" } else { "0x" };
        f.field("k", &format_args!("{prefix}{k:x}")).finish()
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
                    Mode::LEN => return write!(f, "{class}{size} len"),
                    Mode::IND => return write!(f, "{class}{size} [x + {k}]"),
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
            Class::JMP => {
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
            Class::RET => {
                let source = Source::decode(code);
                let size = Size::decode(code);

                return match (source, size) {
                    (Source::K, Size::B) if k == 0 => write!(f, "{class} %a"),
                    (Source::K, _) => write!(f, "{class} #{k}"),
                    (Source::X, _) => write!(f, "{class} %x"),
                };
            }
            Class::MISC => {
                let misc = Misc::decode(code);
                return match misc {
                    Misc::TAX => write!(f, "tax"),
                    Misc::TXA => write!(f, "txa"),
                };
            }
            _ => {}
        }

        write!(f, "<unknown instruction {i:?}>")
    }
}

// https://github.com/torvalds/linux/blob/b947cc5bf6d793101135265352e205aeb30b54f0/include/uapi/linux/bpf_common.h#L6
define!(
    #[mask(0x07)]
    pub enum Class {
        LD = 0x00,
        LDX = 0x01,
        ST = 0x02,
        STX = 0x03,
        ALU = 0x04,
        JMP = 0x05,
        RET = 0x06,
        MISC = 0x07,
    }
);

// https://github.com/torvalds/linux/blob/b947cc5bf6d793101135265352e205aeb30b54f0/include/uapi/linux/bpf_common.h#L17
define!(
    #[mask(0x18)]
    pub enum Size {
        // word (4 bytes)
        W = 0x00,
        // half word (2 bytes)
        H = 0x08,
        // byte
        B = 0x10,
    }
);

impl Size {
    #[inline]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::W => "",
            Self::H => "H",
            Self::B => "B",
        }
    }
}

// https://github.com/torvalds/linux/blob/b947cc5bf6d793101135265352e205aeb30b54f0/include/uapi/linux/bpf_common.h#L22
define!(
    #[mask(0xe0)]
    pub enum Mode {
        IMM = 0x00,
        ABS = 0x20,
        IND = 0x40,
        MEM = 0x60,
        LEN = 0x80,
        MSH = 0xa0,
    }
);

// https://github.com/torvalds/linux/blob/b947cc5bf6d793101135265352e205aeb30b54f0/include/uapi/linux/bpf_common.h#L31
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
    }
);

// https://github.com/torvalds/linux/blob/b947cc5bf6d793101135265352e205aeb30b54f0/include/uapi/linux/bpf_common.h#L44
define!(
    #[mask(0xf0)]
    pub enum Jump {
        JA = 0x00,
        JEQ = 0x10,
        JGT = 0x20,
        JSET = 0x40,
    }
);

define!(
    #[mask(0xf0)]
    pub enum Misc {
        TAX = 0x00,
        TXA = 0x80,
    }
);

impl_ops!();
impl_ret!();

pub const fn len() -> K {
    K {
        mode: Mode::LEN,
        value: 0,
    }
}

pub const fn tax() -> Instruction {
    Instruction::raw(Class::MISC as u16 | Misc::TAX as u16, 0, 0, 0)
}

pub const fn txa() -> Instruction {
    Instruction::raw(Class::MISC as u16 | Misc::TXA as u16, 0, 0, 0)
}
