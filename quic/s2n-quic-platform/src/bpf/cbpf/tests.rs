// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::socket::options::{Options, ReusePort};
use std::{
    io::{self, Write},
    net::UdpSocket,
};

macro_rules! test {
    ($name:ident,no_run, $asm:expr, $instructions:expr $(,)?) => {
        #[test]
        fn $name() {
            Test {
                name: stringify!($name),
                asm: $asm,
                instructions: $instructions,
                checks: Checks {
                    run: false,
                    ..Default::default()
                },
            }
            .check();
        }
    };
    ($name:ident, $asm:expr, $instructions:expr $(,)?) => {
        #[test]
        fn $name() {
            Test {
                name: stringify!($name),
                asm: $asm,
                instructions: $instructions,
                checks: Default::default(),
            }
            .check();
        }
    };
}

test!(
    example_ipv4_tcp_packets,
    r#"
    ldh [12]
    jne #0x800, drop
    ldb [23]
    jneq #6, drop
    ret #-1
    drop: ret #0
    "#,
    &[
        ldh(abs(12)),
        jneq(0x800, 3, 0),
        ldb(abs(23)),
        jneq(6, 1, 0),
        ret(u32::MAX),
        ret(0)
    ],
);

test!(
    example_interface_index_13,
    r#"
    ld ifidx
    jneq #13, drop
    ret #-1
    drop: ret #0
    "#,
    &[
        ld(ancillary::skb::ifindex()),
        jneq(13, 1, 0),
        ret(u32::MAX),
        ret(0),
    ],
);

test!(
    example_vlan_w_id_10,
    r#"
    ld vlan_tci
    jneq #10, drop
    ret #-1
    drop: ret #0
    "#,
    &[
        ld(ancillary::skb::vlan_tci()),
        jneq(10, 1, 0),
        ret(u32::MAX),
        ret(0),
    ],
);

const FIRST_BYTE_MAX: u8 = 0b0011_1111;

test!(
    first_byte_routing,
    &format!(
        r#"
    ldb [0]
    and #{}
    jneq #{}, reader
    ret #0
    reader: ret #1
    "#,
        !FIRST_BYTE_MAX,
        FIRST_BYTE_MAX + 1
    ),
    &[
        // load the first byte of the packet
        ldb(abs(0)),
        // mask off the LSBs
        and(!FIRST_BYTE_MAX as _),
        // IF:
        // the control bit is set
        jneq(FIRST_BYTE_MAX as u32 + 1, 1, 0),
        // THEN:
        // return a 0 indicating we want to route to the writer socket
        ret(0),
        // ELSE:
        // return a 1 indicating we want to route to the reader socket
        ret(1),
    ],
);

test!(
    hash_routing,
    r#"
    ld rxhash
    and #0x1
    ret %a
    "#,
    &[ld(ancillary::skb::hash()), and(1), ret_a(),]
);

test!(
    len_check,
    r#"
    ld len
    jgt #100, drop
    ret #0
    drop: ret #1
    "#,
    &[ld(len()), jgt(100, 1, 0), ret(0), ret(1)]
);

test!(
    comparisons,
    r#"
    ld [0]
    jgt #1, drop
    jle #2, drop
    jeq #3, drop
    jneq #4, drop
    drop: ret #0
    "#,
    &[
        ld(abs(0)),
        jgt(1, 3, 0),
        jle(2, 2, 0),
        jeq(3, 1, 0),
        jneq(4, 0, 0),
        ret(0)
    ]
);

// pkt += match *pkt >> 6 {
//     0 => 1,
//     1 => 2,
//     2 => 4,
//     3 => 8,
// };
test!(
    varint_skip,
    r#"
        ldx #0                        ;; initialize the cursor

    varint_skip:
        ldb [x + 0]                   ;; load the current byte from the cursor
        rsh #6                        ;; shift the first byte into the 2 bit mask
        add #1                        ;; add 1 to the mask
        jgt #2, varint_skip_done      ;; if the mask is 1 or 2, we're done

        add #1                        ;; increment by 1 - either it's 4 or 8
        jeq #4, varint_skip_done      ;; if the mask is 4, we're done

        add #4                        ;; increment by 4, getting us to 8

    varint_skip_done:
        add %x                        ;; add the varint length to the cursor
        tax                           ;; put the new cursor into x
        ret #0                        ;; return 0 just for the test
    "#,
    &[
        // initialize the cursor
        ldx(imm(0)),
        // load the current byte from the cursor
        ldb(ind(0)),
        // shift the first byte into the 2 bit mask
        rsh(6),
        // add 1 to the mask
        add(1),
        // if the mask is 1 or 2, we're done
        jgt(2, 3, 0),
        // increment by 1 - either it's 4 or 8
        add(1),
        // if the mask is 4, we're done
        jeq(4, 1, 0),
        // increment by 4, getting us to 8
        add(4),
        // add the varint length to the cursor
        add_x(),
        // put the new cursor into x
        tax(),
        // return 0 just for the test
        ret(0),
    ]
);

struct Test<'a> {
    name: &'a str,
    asm: &'a str,
    instructions: &'a [Instruction],
    checks: Checks,
}

impl Test<'_> {
    fn check(&self) {
        let prog = Program::new(self.instructions);

        insta::assert_snapshot!(format!("{}_display", self.name), prog);
        insta::assert_debug_snapshot!(format!("{}_debug", self.name), prog);
        let mut actual_c_literal = String::new();
        for i in self.instructions {
            use core::fmt::Write;
            writeln!(actual_c_literal, "{i:#},").unwrap();
        }
        insta::assert_snapshot!(format!("{}_c_literal", self.name), actual_c_literal);

        self.checks.compile(
            &[
                // this ensures the instruction functions actually map to the correct asm
                self.asm,
                // this ensures that the disassembly can reproduce the original asm code
                &prog.to_string(),
            ],
            &actual_c_literal,
        );
        self.checks.run(&prog);
    }
}

struct BpfAsm {
    path: String,
}

impl BpfAsm {
    /// Tries to resolve a `bpf_asm` binary capable of assembling BPF programs.
    ///
    /// If none is found, then `None` is returned.
    fn resolve() -> Option<&'static Self> {
        use std::sync::OnceLock;

        static INSTANCE: OnceLock<Option<BpfAsm>> = OnceLock::new();

        INSTANCE
            .get_or_init(|| {
                let bpf_asm = std::process::Command::new("which")
                    .arg("bpf_asm")
                    .output()
                    .ok()?;

                let bpf_asm = core::str::from_utf8(&bpf_asm.stdout).unwrap_or("").trim();
                if bpf_asm.is_empty() {
                    return None;
                }

                Some(Self {
                    path: bpf_asm.to_string(),
                })
            })
            .as_ref()
    }

    fn assemble(&self, asm: &str) -> String {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(asm.as_bytes()).unwrap();

        let output = std::process::Command::new(&self.path)
            .arg("-c")
            .arg(file.path())
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}\nINPUT:\n{asm}");
        let output = core::str::from_utf8(&output.stdout).unwrap();

        output.to_string()
    }
}

#[derive(Debug)]
struct Checks {
    compile: bool,
    run: bool,
}

impl Default for Checks {
    fn default() -> Self {
        Self {
            compile: true,
            run: true,
        }
    }
}

impl Checks {
    fn compile(&self, asm: &[&str], expected: &str) {
        if !self.compile {
            return;
        }
        let Some(bpf_asm) = BpfAsm::resolve() else {
            eprintln!("`bpf_asm` not found in environment - skipping tests");
            return;
        };

        for asm in asm {
            let out = bpf_asm.assemble(asm);
            assert_eq!(
                expected, out,
                "\nINPUT:\n{asm}\nBPF_OUT:\n{out}\nEXPECTED:\n{expected}\n"
            );
        }
    }

    fn run(&self, prog: &Program) {
        if !self.run {
            return;
        }

        use std::sync::OnceLock;

        static PAIR: OnceLock<io::Result<(UdpSocket, UdpSocket)>> = OnceLock::new();

        let pair = PAIR.get_or_init(|| {
            udp_pair().and_then(|(a, b)| {
                probe(&a)?;
                probe(&b)?;
                Ok((a, b))
            })
        });

        let Ok((a, b)) = pair else {
            eprintln!("skipping tests due to environment not supporting bpf programs");
            return;
        };

        for socket in [a, b] {
            prog.attach(socket).unwrap();
        }
    }
}

fn udp_pair() -> io::Result<(UdpSocket, UdpSocket)> {
    let mut options = Options {
        gro: false,
        blocking: false,
        // set the reuse port option after binding to avoid port collisions
        reuse_port: ReusePort::AfterBind,
        ..Default::default()
    };

    let writer = options.build_udp()?;

    // bind the sockets to the same address
    options.addr = writer.local_addr()?;
    // now that we have a concrete port from the OS, we set the option before the bind call
    options.reuse_port = ReusePort::BeforeBind;

    let reader = options.build_udp()?;

    Ok((writer, reader))
}

/// Probe for BPF support in the environment
fn probe<F: std::os::fd::AsRawFd>(socket: &F) -> io::Result<()> {
    use libc::{sock_filter, sock_fprog};

    // $ echo 'ret #0' | bfp_asm -c
    // { 0x06,  0,  0, 0000000000 },
    let instructions = [sock_filter {
        code: 0x06,
        jt: 0,
        jf: 0,
        k: 0,
    }];

    let prog = sock_fprog {
        filter: instructions.as_ptr() as *const _ as *mut _,
        len: instructions.len() as _,
    };

    let ret = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            <Cbpf as super::super::instruction::Dialect>::SOCKOPT as _,
            &prog as *const _ as *const _,
            core::mem::size_of::<sock_fprog>() as _,
        )
    };

    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
