from glob import glob
import argparse
import json
import sys

start_time = None
end_time = None
quic_draft = None
quic_version = None
diff_regression = False
S2N_QUIC = "s2n-quic"
S2N_QUIC_DIFF = S2N_QUIC + "-diff"
s2n_quic_new_version_name = S2N_QUIC
s2n_quic_prev_version_name = S2N_QUIC
clients = set()
servers = set()
results = {}
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


# Sort the implementations, except for s2n-quic being ordered as
# previous version, new version, diff
def reorder_implementations(impls):
    sorted_impls = sorted(impls)

    if args.prev_version and S2N_QUIC in sorted_impls:
        sorted_impls.remove(s2n_quic_prev_version_name)
        sorted_impls.remove(s2n_quic_new_version_name)
        sorted_impls.remove(S2N_QUIC_DIFF)

        for i, impl in enumerate(sorted_impls):
            if impl > S2N_QUIC:
                sorted_impls.insert(i, s2n_quic_prev_version_name)
                sorted_impls.insert(i + 1, s2n_quic_new_version_name)
                sorted_impls.insert(i + 2, S2N_QUIC_DIFF)
                break

    return sorted_impls


parser = argparse.ArgumentParser(description='Merge interop reports.')
parser.add_argument('--new_version_suffix')
parser.add_argument('--new_version_url')
parser.add_argument('--new_version_log_url')
parser.add_argument('--prev_version_suffix')
parser.add_argument('--prev_version_url')
parser.add_argument('--prev_version_log_url')
parser.add_argument('--prev_version')
parser.add_argument('--interop_log_url')
parser.add_argument('patterns', nargs='+')
args = parser.parse_args()

if args.new_version_suffix:
    s2n_quic_new_version_name += '-' + args.new_version_suffix
if args.prev_version_suffix:
    s2n_quic_prev_version_name += '-' + args.prev_version_suffix
if args.new_version_url:
    urls[s2n_quic_new_version_name] = args.new_version_url
if args.prev_version_url:
    urls[s2n_quic_prev_version_name] = args.prev_version_url
if args.new_version_log_url:
    logs[s2n_quic_new_version_name] = args.new_version_log_url
if args.prev_version_log_url:
    logs[s2n_quic_prev_version_name] = args.prev_version_log_url

for pattern in args.patterns:
    for path in glob(pattern, recursive = True):
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
                # Rename the previous version of s2n-quic (used when pushing to main)
                client = s2n_quic_prev_version_name
                results.setdefault(S2N_QUIC_DIFF, {})
                measurements.setdefault(S2N_QUIC_DIFF, {})
                clients.add(S2N_QUIC_DIFF)

            results.setdefault(client, {})
            measurements.setdefault(client, {})

            clients.add(client)

            for server in result.get('servers', []):
                if server == S2N_QUIC:
                    # Rename the previous version of s2n-quic (used when pushing to main)
                    server = s2n_quic_prev_version_name
                    results[client].setdefault(S2N_QUIC_DIFF, {})
                    measurements[client].setdefault(S2N_QUIC_DIFF, {})
                    servers.add(S2N_QUIC_DIFF)

                # We only need to compare to s2n-quic
                if client != s2n_quic_prev_version_name and server != s2n_quic_prev_version_name:
                    index += 1
                    continue
                    
                servers.add(server)

                # Don't consider different versions of s2n-quic against each other
                if client.startswith(S2N_QUIC) and server.startswith(S2N_QUIC) and client != server:
                    index += 1
                    continue

                results[client].setdefault(server, {})
                measurements[client].setdefault(server, {})

                for r in result['results'][index]:
                    test = r['abbr']
                    prev_result = r['result']
                    diff_output = None
                    results[client][server][test] = prev_result

                    if client == s2n_quic_prev_version_name and server == s2n_quic_prev_version_name:
                        # diff with new versions of both the s2n-quic client and server
                        diff_output = diff_result(s2n_quic_new_version_name, s2n_quic_new_version_name, test, prev_result)
                        results[S2N_QUIC_DIFF][S2N_QUIC_DIFF][test] = diff_output
                    elif server == s2n_quic_prev_version_name:
                        # diff with the new version of the s2n-quic server
                        diff_output = diff_result(client, s2n_quic_new_version_name, test, prev_result)
                        results[client][S2N_QUIC_DIFF][test] = diff_output
                    elif client == s2n_quic_prev_version_name:
                        # diff with the new version of the s2n-quic client
                        diff_output = diff_result(s2n_quic_new_version_name, server, test, prev_result)
                        results[S2N_QUIC_DIFF][server][test] = diff_output

                    if prev_result == 'succeeded' and diff_output == 'failed':
                        # If any test went from success to failure, count as a regression
                        diff_regression = True

                for m in result['measurements'][index]:
                    measurements[client][server][m['abbr']] = m
                    # TODO diff measurements

                index += 1

out = {
    "start_time": start_time,
    "end_time": end_time,
    "log_dir": args.interop_log_url,
    "s2n_quic_log_dir": logs,
    "servers": reorder_implementations(servers),
    "clients": reorder_implementations(clients),
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

print(json.dumps(out))

if diff_regression:
    sys.exit(1)
