#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

from glob import glob
import argparse
import json

start_time = None
end_time = None
quic_draft = None
quic_version = None
regression = False
S2N_QUIC = "s2n-quic"
s2n_quic_new_version_name = S2N_QUIC
clients = set()
servers = set()
results = {}
results_diff = {}
measurements = {}
tests = {}
logs = {}

urls = {}

def update_value(res, key, prev, cmp):
    value = res.get(key)
    if prev:
        if value:
            return cmp(value, prev)
        else:
            return prev
    else:
        return value

def strict_eq(x, y):
    if x != y:
        raise "version does not match"
    return x

# Retrieve the result for the given client and server, returning
# the new result if it defers from the previous result.
def diff_result(client, server, test, prev_result):
    new_result = results.get(client, {}).get(server, {}).get(test, {})

    if new_result != prev_result:
        return new_result
    return {}


parser = argparse.ArgumentParser(description='Merge interop reports.')
parser.add_argument('--new_version_suffix')
parser.add_argument('--new_version_url')
parser.add_argument('--new_version_log_url')
parser.add_argument('--prev_version_log_url')
parser.add_argument('--prev_version')
parser.add_argument('--prev_version_url')
parser.add_argument('--interop_log_url')
parser.add_argument('patterns', nargs='+')
args = parser.parse_args()

if args.new_version_suffix:
    s2n_quic_new_version_name += '-' + args.new_version_suffix.lower()
if args.new_version_log_url:
    logs[s2n_quic_new_version_name] = args.new_version_log_url
if args.prev_version_log_url:
    logs[S2N_QUIC] = args.prev_version_log_url

for pattern in args.patterns:
    for path in glob(pattern, recursive=True):
        with open(path) as f:
            result = json.load(f)
            index = 0

            start_time = update_value(result, 'start_time', start_time, min)
            end_time = update_value(result, 'end_time', end_time, max)

            quic_draft = update_value(result, 'quic_draft', quic_draft, strict_eq)
            quic_version = update_value(result, 'quic_version', quic_version, strict_eq)

            tests.update(result.get('tests', {}))
            urls.update(result.get('urls', {}))

            for client in result.get('clients', []):
                # Rename the new version of s2n-quic (used with pull requests)
                if client == S2N_QUIC:
                    client = s2n_quic_new_version_name

                results.setdefault(client, {})
                measurements.setdefault(client, {})

                clients.add(client)

                for server in result.get('servers', []):
                    # Rename the new version of s2n-quic (used with pull requests)
                    if server == S2N_QUIC:
                        server = s2n_quic_new_version_name

                    servers.add(server)
 
                    results[client].setdefault(server, {})
                    measurements[client].setdefault(server, {})

                    for r in result['results'][index]:
                        results[client][server][r['abbr']] = r['result']

                    for m in result['measurements'][index]:
                        measurements[client][server][m['abbr']] = m

                    index += 1

# If prev_version argument is supplied
if args.prev_version:
    with open(args.prev_version) as f:
        result = json.load(f)
        index = 0

        for client in result.get('clients', []):
            if client == S2N_QUIC:
                results_diff.setdefault(S2N_QUIC, {})

            results.setdefault(client, {})
            measurements.setdefault(client, {})
            results_diff.setdefault(client, {})

            clients.add(client)

            for server in result.get('servers', []):
                if server == S2N_QUIC:
                    results_diff[client].setdefault(S2N_QUIC, {})

                # We only need to compare to s2n-quic
                if client != S2N_QUIC and server != S2N_QUIC:
                    index += 1
                    continue
                    
                servers.add(server)

                results[client].setdefault(server, {})
                measurements[client].setdefault(server, {})
                results_diff[client].setdefault(server, {})

                for r in result['results'][index]:
                    test = r['abbr']
                    prev_result = r['result']
                    diff_output = None
                    results[client][server][test] = prev_result

                    if client == S2N_QUIC and server == S2N_QUIC:
                        # diff with new versions of both the s2n-quic client and server
                        diff_output = diff_result(s2n_quic_new_version_name, s2n_quic_new_version_name, test, prev_result)
                        results_diff[S2N_QUIC][S2N_QUIC][test] = diff_output
                    elif server == S2N_QUIC:
                        # diff with the new version of the s2n-quic server
                        diff_output = diff_result(client, s2n_quic_new_version_name, test, prev_result)
                        results_diff[client][S2N_QUIC][test] = diff_output
                    elif client == S2N_QUIC:
                        # diff with the new version of the s2n-quic client
                        diff_output = diff_result(s2n_quic_new_version_name, server, test, prev_result)
                        results_diff[S2N_QUIC][server][test] = diff_output

                    if prev_result == 'succeeded' and diff_output == 'failed':
                        # If any test went from success to failure, count as a regression
                        regression = True

                for m in result['measurements'][index]:
                    measurements[client][server][m['abbr']] = m
                    # TODO diff measurements

                index += 1

# Update s2n-quic urls
if args.new_version_url:
    urls[s2n_quic_new_version_name] = args.new_version_url
if args.prev_version_url:
    urls[S2N_QUIC] = args.prev_version_url

out = {
    "start_time": start_time,
    "end_time": end_time,
    "log_dir": args.interop_log_url,
    "s2n_quic_log_dir": logs,
    "servers": sorted(servers),
    "clients": sorted(clients),
    "urls": urls,
    "tests": tests,
    "quic_draft": quic_draft,
    "quic_version": quic_version,
    "results": [],
    "measurements": [],
}

for client in out['clients']:
    for server in out['servers']:
        pair_results = []
        pair_measurements = []

        for test in sorted(tests.keys()):
            r = results.get(client, {}).get(server, {}).get(test)
            if r:
                pair_results.append(
                    {
                        "abbr": test,
                        "name": tests[test]['name'],
                        "result": r,
                    }
                )

            r = measurements.get(client, {}).get(server, {}).get(test)
            if r:
                pair_measurements.append(r)

        out["results"].append(pair_results)
        out["measurements"].append(pair_measurements)

if args.prev_version:
    impls = clients | servers
    impls.discard(s2n_quic_new_version_name)
    impls.add(S2N_QUIC)
    out['all_impls'] = sorted(impls)
    out['new_version'] = s2n_quic_new_version_name
    out['prev_version'] = S2N_QUIC
    out['regression'] = regression
    out.setdefault("results_diff", {})

    for server in results_diff.get(S2N_QUIC, {}):
        if len(results_diff.get(S2N_QUIC, {}).get(server, {})) > 0:
            out["results_diff"].setdefault('client', [])
            break

    for client in results_diff:
        if len(results_diff.get(client, {}).get(S2N_QUIC, {})) > 0:
            out["results_diff"].setdefault('server', [])
            break

    for impl in out['all_impls']:
        pair_results = []

        if 'server' in out["results_diff"]:
            for test in sorted(tests.keys()):
                server_diff = results_diff.get(impl, {}).get(S2N_QUIC, {}).get(test)
                if server_diff:
                    pair_results.append(
                        {
                            "abbr": test,
                            "name": tests[test]['name'],
                            "result": server_diff,
                        }
                    )
            out["results_diff"]["server"].append(pair_results)

        if 'client' in out["results_diff"]:
            for test in sorted(tests.keys()):
                client_diff = results_diff.get(S2N_QUIC, {}).get(impl, {}).get(test)
                if client_diff:
                    pair_results.append(
                        {
                            "abbr": test,
                            "name": tests[test]['name'],
                            "result": client_diff,
                        }
                    )
            out["results_diff"]["client"].append(pair_results)

print(json.dumps(out))
