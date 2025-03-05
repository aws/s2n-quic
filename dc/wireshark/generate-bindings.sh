#!/usr/bin/env bash

set -xeuo pipefail

VERSION="4.4.2"
BRANCH="wireshark-$VERSION"
PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-}"

# Install bindgen...
if ! command -v bindgen &> /dev/null; then
    cargo +stable install bindgen-cli
fi

INCLUDES=()

nixpath() {
    nix-shell --packages $1 --run 'echo -n $buildInputs'
}

# add nix-specific paths
if command -v nix-shell &> /dev/null; then
  PKG_CONFIG_PATH="$(nixpath wireshark.dev)/lib/pkgconfig:$(nixpath glib.dev)/lib/pkgconfig:$PKG_CONFIG_PATH"
elif command -v brew &> /dev/null; then
  brew install pkg-config wireshark
elif command -v apt-get &> /dev/null; then
  sudo add-apt-repository ppa:wireshark-dev/stable
  sudo apt-get update
  sudo apt-get install pkg-config wireshark-dev tshark -y
fi

INCLUDES=(
  "$(PKG_CONFIG_PATH="$PKG_CONFIG_PATH" pkg-config --cflags-only-I glib-2.0 wireshark)"
)

OPTIONS=(
  --allowlist-type 'gint'
  --allowlist-type 'guint'
  --allowlist-type 'guint16'
  --allowlist-type 'guint32'
  --allowlist-type 'gboolean'
  --allowlist-type 'nstime_t'
  --allowlist-type '_packet_info'
  --allowlist-type '_header_field_info'
  --opaque-type 'frame_data'
  --opaque-type '_proto_node'
  --allowlist-type 'frame_data'
  --allowlist-type '_proto_node'
  --allowlist-type 'proto_plugin'
  --opaque-type 'epan_column_info'
  --allowlist-type 'epan_column_info'
  --opaque-type 'tvbuff'
  --allowlist-type 'tvbuff'
  --opaque-type 'tvbuff_t'
  --allowlist-type 'tvbuff_t'
  --opaque-type 'address'
  --allowlist-type 'address'
  --opaque-type 'port_type'
  --allowlist-type 'port_type'
  --opaque-type 'GSList'
  --allowlist-type 'GSList'
  --opaque-type 'GHashTable'
  --allowlist-type 'GHashTable'
  --opaque-type 'wtap_pseudo_header'
  --allowlist-type 'wtap_pseudo_header'
  --opaque-type 'wtap_rec'
  --allowlist-type 'wtap_rec'
  --opaque-type 'conversation_addr_port_endpoints'
  --allowlist-type 'conversation_addr_port_endpoints'
  --opaque-type 'conversation_element'
  --allowlist-type 'conversation_element'
  --allowlist-type 'dissector_handle_t'
  --allowlist-type 'ftenum_t'
  --allowlist-type 'field_display_e'
  --allowlist-var 'COL_PROTOCOL'
  --allowlist-var 'ENC_BIG_ENDIAN'
  --allowlist-var 'DESEGMENT_ONE_MORE_SEGMENT'
  --allowlist-var 'DESEGMENT_UNTIL_FIN'
)

mkdir -p src/wireshark_sys/

RUST_TARGET=$(rustc -vV | grep release: | awk '{ print $2 }')

# This list is filtered to roughly what our current usage requires.
# It's possible there's a better way to do this -- some of the Wireshark
# headers end up pulling in C++ so we do need some filtering.
bindgen \
  --rust-target $RUST_TARGET \
  --raw-line '// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.' \
  --raw-line '// SPDX-License-Identifier: Apache-2.0' \
  ${OPTIONS[@]} \
  wrapper.h \
  -o src/wireshark_sys/minimal.rs \
  -- ${INCLUDES[@]}

bindgen \
  --rust-target $RUST_TARGET \
  --raw-line '// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.' \
  --raw-line '// SPDX-License-Identifier: Apache-2.0' \
  ${OPTIONS[@]} \
  --allowlist-function 'proto_register_.*' \
  --allowlist-function 'proto_tree_.*' \
  --allowlist-function 'proto_item_.*' \
  --allowlist-function 'tvb_memcpy' \
  --allowlist-function 'tvb_reported_length' \
  --allowlist-function 'tvb_reported_length' \
  --allowlist-function 'heuristic_.*' \
  --allowlist-function 'heur.*' \
  --allowlist-function 'create_dissector_handle_with_name_and_description' \
  --allowlist-function 'col_set_str' \
  --allowlist-function 'col_append_str' \
  --allowlist-function 'col_clear' \
  --allowlist-function 'find_or_create_conversation' \
  --allowlist-function 'conversation_set_dissector' \
  wrapper.h \
  -o src/wireshark_sys/full.rs \
  -- ${INCLUDES[@]}
