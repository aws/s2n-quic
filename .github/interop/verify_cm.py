#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

from glob import glob
import argparse
import json

VERIFY_STRING = 'update active path'
S2N_QUIC = "s2n-quic"
addr_success = set()
port_success = set()

parser = argparse.ArgumentParser(description='Merge interop reports.')
parser.add_argument('--log_path')
parser.add_argument('--clients')
args = parser.parse_args()

print(args)

def check_addr(args):
    for client in args.clients:
        file = open(f'{args.log_path}/{client}/rebind-addr/logs.txt', "r")
        readfile = file.read()

        if VERIFY_STRING in readfile:
            addr_success.add(client)
        file.close()

def check_port(args):
    for client in args.clients:
        file = open(f'{args.log_path}/{client}/rebind-port/logs.txt', "r")
        readfile = file.read()

        if VERIFY_STRING in readfile:
            port_success.add(client)
        file.close()

check_addr(args)
check_port(args)

print(addr_success)
print(port_success)
