#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

from glob import glob
import argparse
import json

# start_time = None
# end_time = None
# quic_draft = None
# quic_version = None
S2N_QUIC = "s2n-quic"
clients = set()
# servers = set()
# results = {}
# results_diff = {}
# measurements = {}
# tests = {}
# logs = {}

# urls = {}

parser = argparse.ArgumentParser(description='Merge interop reports.')
parser.add_argument('--new_version_suffix')
parser.add_argument('--new_version_url')
parser.add_argument('--new_version_log_url')
parser.add_argument('--interop_log_url')
parser.add_argument('patterns', nargs='+')
args = parser.parse_args()

out = {
    "clients": sorted(clients),
}

def parse_clients(args):
    for pattern in args.patterns:
        for path in glob(pattern, recursive=True):
            with open(path) as f:
                result = json.load(f)
                for client in result.get('clients', []):
                    clients.add(client)


def check_for_success():
    for client in out['clients']:
        file = 
        file.contains("update active path")

