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
        impl_ld!(ld, ldx, W);
        impl_ld!(ldb, ldbx, B);
        impl_ld!(ldh, ldhx, H);
    };
    ($ld:ident, $x:ident, $size:ident) => {
        pub const fn $ld(k: K) -> Instruction {
            Instruction::raw(
                k.mode as u16 | Class::LD as u16 | Size::$size as u16,
                0,
                0,
                k.value,
            )
        }

        pub const fn $x(k: K) -> Instruction {
            Instruction::raw(
                k.mode as u16 | Class::LDX as u16 | Size::$size as u16,
                0,
                0,
                k.value,
            )
        }
    };
}

macro_rules! impl_alu {
    () => {
        impl_alu!(add, add_x, ADD);
        impl_alu!(sub, sub_x, SUB);
        impl_alu!(mul, mul_x, MUL);
        impl_alu!(div, div_x, DIV);
        impl_alu!(rem, rem_x, MOD);
        impl_alu!(and, and_x, AND);
        impl_alu!(or, or_x, OR);
        impl_alu!(xor, xor_x, XOR);
        impl_alu!(lsh, lsh_x, LSH);
        impl_alu!(rsh, rsh_x, RSH);
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

        impl_jmp!(jgt, jgt_x, JGT, false);
        impl_jmp!(jle, jle_x, JGT, true);
        impl_jmp!(jeq, jeq_x, JEQ, false);
        impl_jmp!(jneq, jneq_x, JEQ, true);
        impl_jmp!(jset, jset_x, JSET, false);
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

        pub const fn ret_x() -> Instruction {
            Instruction::raw(Class::RET as u16 | Source::X as u16, 0, 0, 0)
        }

        pub const fn ret_a() -> Instruction {
            Instruction::raw(Class::RET as u16 | Size::B as u16, 0, 0, 0)
        }
    };
}

macro_rules! impl_ops {
    () => {
        #[derive(Clone, Copy)]
        pub struct K {
            pub mode: Mode,
            pub value: u32,
        }

        pub const fn abs(value: u32) -> K {
            K {
                mode: Mode::ABS,
                value,
            }
        }

        pub const fn imm(value: u32) -> K {
            K {
                mode: Mode::IMM,
                value,
            }
        }

        pub const fn ind(value: u32) -> K {
            K {
                mode: Mode::IND,
                value,
            }
        }

        impl_ld!();
        impl_alu!();
        impl_jmp!();
        impl_ancillary!();
    };
}
