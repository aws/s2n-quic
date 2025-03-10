// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Ancillary data extensions are used to look up additional information about a packet
//!
//! See <https://docs.kernel.org/networking/filter.html#bpf-engine-and-instruction-set>

macro_rules! skf_value {
    ($name:ident) => {
        (libc::SKF_AD_OFF + libc::$name) as _
    };
}

#[derive(Clone, Copy, Debug)]
pub struct Info {
    /// The special extension name used in `bpf_asm`
    pub extension: &'static str,
    /// The `C` interface
    pub capi: &'static str,
}

/// Returns the [`Info`] for a given offset
pub const fn lookup(offset: u32) -> Option<Info> {
    macro_rules! map {
        ($(($value:expr, $extension:expr, $capi:expr),)*) => {
            match offset {
                $(_ if offset == $value => Some(Info { extension: $extension, capi: $capi }),)*
                _ => None,
            }
        };
    }

    map!(
        (skb::protocol(), "proto", "skb->protocol"),
        (skb::pkt_type(), "type", "skb->pkt_type"),
        (skb::ifindex(), "ifidx", "skb->dev->ifindex"),
        (skb::mark(), "mark", "skb->mark"),
        (skb::queue_mapping(), "queue", "skb->queue_mapping"),
        (skb::dev_type(), "hatype", "skb->dev->type"),
        (skb::hash(), "rxhash", "skb->hash"),
        (skb::vlan_tci(), "vlan_tci", "skb_vlan_tag_get(skb)"),
        (skb::vlan_avail(), "vlan_avail", "skb_vlan_tag_present(skb)"),
        (skb::vlan_proto(), "vlan_tpid", "skb->vlan_tproto"),
        (payload_offset(), "poff", "payload_offset()"),
        (raw_smp_processor_id(), "cpu", "raw_smp_processor_id()"),
        (get_random_u32(), "rand", "get_random_u32()"),
    )
}

pub mod skb {
    macro_rules! skb_value {
        ($name:ident, $value:ident) => {
            #[inline]
            pub const fn $name() -> u32 {
                skf_value!($value)
            }
        };
    }

    skb_value!(protocol, SKF_AD_PROTOCOL);
    skb_value!(pkt_type, SKF_AD_PKTTYPE);
    skb_value!(ifindex, SKF_AD_IFINDEX);
    skb_value!(mark, SKF_AD_MARK);
    skb_value!(queue_mapping, SKF_AD_QUEUE);
    skb_value!(dev_type, SKF_AD_HATYPE);
    skb_value!(hash, SKF_AD_RXHASH);
    skb_value!(vlan_tci, SKF_AD_VLAN_TAG);
    skb_value!(vlan_avail, SKF_AD_VLAN_TAG_PRESENT);
    skb_value!(vlan_proto, SKF_AD_VLAN_TPID);
}

#[inline]
pub const fn payload_offset() -> u32 {
    skf_value!(SKF_AD_PAY_OFFSET)
}

#[inline]
pub const fn raw_smp_processor_id() -> u32 {
    skf_value!(SKF_AD_CPU)
}

#[inline]
pub const fn get_random_u32() -> u32 {
    skf_value!(SKF_AD_RANDOM)
}

macro_rules! impl_ancillary {
    () => {
        /// Ancillary data extensions are used to look up additional information about a packet
        ///
        /// See <https://docs.kernel.org/networking/filter.html#bpf-engine-and-instruction-set>
        pub mod ancillary {
            use super::{super::ancillary, *};

            /// Data associated with the socket buffer (skb)
            pub mod skb {
                use super::{ancillary::skb, *};

                /// Loads the `skb->len` into the `A` register
                pub const fn len() -> K {
                    // use the dialect-specific instruction to load the skb len
                    //
                    // in the case of CBPF, there is a single `Mode` for `LEN`
                    super::super::len()
                }

                /// Loads the `skb->protocol` into the `A` register
                pub const fn protocol() -> K {
                    abs(skb::protocol())
                }

                /// Loads the `skb->pkt_type` into the `A` register
                pub const fn pkt_type() -> K {
                    abs(skb::pkt_type())
                }

                /// Loads the `skb->ifindex` into the `A` register
                pub const fn ifindex() -> K {
                    abs(skb::ifindex())
                }

                /// Loads the `skb->mark` into the `A` register
                pub const fn mark() -> K {
                    abs(skb::mark())
                }

                /// Loads the `skb->queue_mapping` into the `A` register
                pub const fn queue_mapping() -> K {
                    abs(skb::queue_mapping())
                }

                /// Loads the `skb->dev->type` into the `A` register
                pub const fn dev_type() -> K {
                    abs(skb::dev_type())
                }

                /// Loads the `skb->hash` into the `A` register
                pub const fn hash() -> K {
                    abs(skb::hash())
                }

                /// Loads the VLAN Tag value into the `A` register
                pub const fn vlan_tci() -> K {
                    abs(skb::vlan_tci())
                }

                /// Loads the VLAN Tag value into the `A` register
                ///
                /// This is used for compatibility with the C API
                pub const fn vlan_tag_get() -> K {
                    vlan_tci()
                }

                /// Loads if the VLAN Tag is present into the `A` register
                pub const fn vlan_avail() -> K {
                    abs(skb::vlan_avail())
                }

                /// Loads if the VLAN Tag is present into the `A` register
                ///
                /// This is used for compatibility with the C API
                pub const fn vlan_tag_present() -> K {
                    vlan_avail()
                }

                /// Loads the `skb->vlan_proto` (VLAN Protocol) into the `A` register
                pub const fn vlan_proto() -> K {
                    abs(skb::vlan_proto())
                }
            }

            /// Loads the payload offset into the `A` register
            pub const fn payload_offset() -> K {
                abs(ancillary::payload_offset())
            }

            /// Loads the CPU ID into the `A` register
            pub const fn raw_smp_processor_id() -> K {
                abs(ancillary::raw_smp_processor_id())
            }

            /// Loads a random `u32` into the `A` register
            pub const fn get_random_u32() -> K {
                abs(ancillary::get_random_u32())
            }
        }
    };
}
