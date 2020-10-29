from glob import glob
import json
import sys

start_time = None
end_time = None
quic_draft = None
quic_version = None
clients = set()
servers = set()
results = {}
measurements = {}
tests = {}

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

for pattern in sys.argv[1:]:
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
                results.setdefault(client, {})
                measurements.setdefault(client, {})

                clients.add(client)

                for server in result.get('servers', []):
                    servers.add(server)
 
                    results[client].setdefault(server, {})
                    measurements[client].setdefault(server, {})

                    for r in result['results'][index]:
                        results[client][server][r['abbr']] = r['result']

                    for m in result['measurements'][index]:
                        measurements[client][server][m['abbr']] = m

                    index += 1

out = {
    "start_time": start_time,
    "end_time": end_time,
    "log_dir": "logs", # TODO how do we merge this?
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

print(json.dumps(out))
