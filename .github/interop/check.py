#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

import argparse
import json
import itertools
import sys

parser = argparse.ArgumentParser(description='Check interop reports.')
parser.add_argument('--required', type=str)
parser.add_argument('--regressions')
parser.add_argument('report')
args = parser.parse_args()

status = {'ok': True, 'failures': 0}
def fail(message):
    print(message)
    status['ok'] = False
    status['failures'] += 1

def format_required_report(report):
    outcome = {}
    result_idx = 0

    # find the version of s2n-quic to check
    s2n_quic = 's2n-quic'
    for impl in itertools.chain(report['clients'], report['servers']):
        # if we're testing a PR then use that name
        if impl.startswith('s2n-quic-pr'):
            s2n_quic = impl
            break

    for client in report['clients']:
        for server in report['servers']:
            result = report['results'][result_idx]
            result_idx += 1

            # we're only interested in s2n-quic results
            if client != s2n_quic and server != s2n_quic:
                continue

            for test in result:
                outcome.setdefault(test['name'], {})

                info = outcome[test['name']]
                if client == s2n_quic and server == s2n_quic:
                    info.setdefault('s2n-quic', {})
                else:
                    info.setdefault(client, {})
                    info.setdefault(server, {})

                success = test['result'] == 'succeeded'
                if client == s2n_quic and server == s2n_quic:
                    info['s2n-quic']['client'] = success
                    info['s2n-quic']['server'] = success
                elif client == s2n_quic:
                    info[server]['server'] = success
                else:
                    info[client]['client'] = success

    return outcome

with open(args.report) as f:
    result = json.load(f)

    if args.regressions and result['regression']:
        fail("A regression from main was detected")

    if args.required:
        with open(args.required) as r:
            required = json.load(r)

            actual = format_required_report(result)

            for test, impls in required.items():
                test_results = actual[test]
                for impl_name, endpoints in impls.items():
                    impl = test_results[impl_name]
                    for endpoint in endpoints:
                        if not impl[endpoint]:
                            fail("{} ({}) - {} was expected to pass but failed".format(impl_name, endpoint, test))

if not status['ok']:
    sys.exit(status['failures'])
