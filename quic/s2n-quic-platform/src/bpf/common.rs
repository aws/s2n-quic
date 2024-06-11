// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// https://github.com/torvalds/linux/blob/b947cc5bf6d793101135265352e205aeb30b54f0/include/uapi/linux/bpf_common.h#L49
define!(
    #[mask(0x08)]
    pub enum Source {
        K = 0x00,
        X = 0x08,
    }
);

macro_rules! impl_ld {
    () => {
        impl_ld!(ld, ldk, W);
        impl_ld!(ldb, ldbk, B);
        impl_ld!(ldh, ldhk, H);
    };
    ($lower:ident, $k:ident, $size:ident) => {
        pub const fn $lower(value: u32) -> Instruction {
            Instruction::raw(
                Mode::ABS as u16 | Class::LD as u16 | Size::$size as u16,
                0,
                0,
                value,
            )
        }

        pub const fn $k(value: u32) -> Instruction {
            Instruction::raw(
                Mode::IMM as u16 | Class::LD as u16 | Source::K as u16 | Size::$size as u16,
                0,
                0,
                value,
            )
        }
    };
}

macro_rules! impl_alu {
    () => {
        impl_alu!(add, addx, ADD);
        impl_alu!(sub, subx, SUB);
        impl_alu!(mul, mulx, MUL);
        impl_alu!(div, divx, DIV);
        impl_alu!(rem, remx, MOD);
        impl_alu!(and, andx, AND);
        impl_alu!(or, orx, OR);
        impl_alu!(xor, xorx, XOR);
        impl_alu!(lsh, lshx, LSH);
        impl_alu!(rsh, rshx, RSH);
    };
    ($lower:ident, $x:ident, $upper:ident) => {
        pub const fn $lower(value: u32) -> Instruction {
            Instruction::raw(
                Mode::IMM as u16 | Class::ALU as u16 | Source::K as u16 | Alu::$upper as u16,
                0,
                0,
                value,
            )
        }

        pub const fn $x() -> Instruction {
            Instruction::raw(
                Mode::IMM as u16 | Class::ALU as u16 | Source::X as u16 | Alu::$upper as u16,
                0,
                0,
                0,
            )
        }
    };
}

macro_rules! impl_jmp {
    () => {
        pub const fn ja(value: u32) -> Instruction {
            Instruction::raw(
                Class::JMP as u16 | Source::K as u16 | Jump::JA as u16,
                0,
                0,
                value,
            )
        }

        impl_jmp!(jgt, jgtx, JGT, false);
        impl_jmp!(jle, jlex, JGT, true);
        impl_jmp!(jeq, jeqx, JEQ, false);
        impl_jmp!(jneq, jneqx, JEQ, true);
        impl_jmp!(jset, jsetx, JSET, false);
    };
    ($lower:ident, $x:ident, $upper:ident, $invert:expr) => {
        pub const fn $lower(value: u32, mut jt: u8, mut jf: u8) -> Instruction {
            if $invert {
                let tmp = jt;
                jt = jf;
                jf = tmp;
            }

            Instruction::raw(
                Class::JMP as u16 | Source::K as u16 | Jump::$upper as u16,
                jt,
                jf,
                value,
            )
        }

        pub const fn $x(mut jt: u8, mut jf: u8) -> Instruction {
            if $invert {
                let tmp = jt;
                jt = jf;
                jf = tmp;
            }

            Instruction::raw(
                Class::JMP as u16 | Source::X as u16 | Jump::$upper as u16,
                jt,
                jf,
                0,
            )
        }
    };
}

macro_rules! impl_ret {
    () => {
        pub const fn ret(value: u32) -> Instruction {
            Instruction::raw(Class::RET as u16 | Source::K as u16, 0, 0, value)
        }

        pub const fn retx() -> Instruction {
            Instruction::raw(Class::RET as u16 | Source::X as u16, 0, 0, 0)
        }
    };
}

macro_rules! impl_ops {
    () => {
        impl_ld!();
        impl_alu!();
        impl_jmp!();
    };
}
