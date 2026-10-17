"""Summarize real OBS cold reconnect evidence without embedding credentials."""
import argparse
import collections
import json
import statistics
from pathlib import Path


def distribution(values):
    values = sorted(values)
    return {'n': len(values), 'median': statistics.median(values) if values else None,
            'p95': values[int(len(values) * .95)] if values else None,
            'min': min(values) if values else None, 'max': max(values) if values else None}


def summarize(root):
    def read(name):
        return json.loads((root / name).read_text(encoding='utf-8'))
    a, n, samples = read('reconnect-analysis.json'), read('native-report.json'), read('samples.json')
    groups = {}
    for name, predicate in [('warm', lambda d: d < 5), ('cold_bridge', lambda d: 5 <= d < 30),
                            ('cold_expired', lambda d: d >= 30)]:
        rows = [r for r in a['intervals'] if predicate(r['on_s'] - r['off_s'])]
        groups[name] = {kind: {
            'obs_ms': distribution([r['outputs'][kind]['obs_recovery_ms'] for r in rows]),
            'native_ms': distribution([r['outputs'][kind]['native_recovery_ms'] for r in rows]),
        } for kind in ['display', 'history']}
    life = n.get('lifecycle', [])
    off = [x['ms'] for x in n['summary']['actions']
           if x['command'] == {'op': 'outputs', 'enabled': False}]
    expiries = [x['ms'] - max(t for t in off if t <= x['ms'])
                for x in life if x['event'] == 'bridge-expired']
    cpu = {event: distribution([x['cpu_ms'] for x in life if x['event'] == event])
           for event in ['bridge-created', 'sender-created', 'surface-prepared']}
    cold = [r for r in a['intervals'] if 5 <= r['on_s'] - r['off_s'] < 30]
    repeated = [s for s in samples if cold and cold[min(1, len(cold)-1)]['on_s'] <= s['t'] <= cold[-1]['on_s'] + 1.5]
    resources = {}
    for key in ['app_private', 'app_handles', 'app_threads', 'obs_private', 'obs_handles']:
        factor = 1048576 if 'private' in key else 1
        resources[key] = {'unit': 'MiB' if factor > 1 else 'count',
                          'repeated_cold': distribution([s[key] / factor for s in repeated]),
                          'final': samples[-1][key] / factor}
    last = {}
    for s in n['sends']:
        last[s['window'], s['generation']] = s
    sender_counts = collections.Counter(s['window'] for s in last.values())
    return {'root': str(root), 'video': a['video'], 'decoded_frames': a['decoded_frames'],
            'recovery': groups, 'lifecycle_counts': dict(collections.Counter(x['event'] for x in life)),
            'lifecycle_cpu_ms': cpu, 'bridge_expiry_after_off_ms': expiries,
            'native_sends': n['summary']['sends'], 'native_releases': n['summary']['retired'],
            'native_pending': n['summary']['pending'], 'native_errors': n['summary']['errors'],
            'sdk_failed': sum(x['failed'] for x in last.values()),
            'sdk_skipped': sum(x['skipped'] for x in last.values()),
            'released_without_copy': sum(x['released_without_copy'] for x in last.values()),
            'pool_generations': dict(sender_counts),
            'max_pool_opens_per_sender': max(x['pool_opens'] for x in n['sends']),
            'release_observed_ms': distribution(n['release_observed_ms']),
            'shader_cache_final': n['summary']['shader_cache'],
            'later_blank_frames': sum(x['later_blank_frames'] for r in a['intervals'] for x in r['outputs'].values()),
            'warm_same_pool': all(x['same_pool'] for r in a['intervals'] if r['warm'] for x in r['outputs'].values()),
            'obs_render_skipped_delta': a['obs_render_skipped_delta'],
            'obs_output_skipped': a['obs_output_skipped'],
            'registration_checks': [{'label': x['label'], 'probe_senders': len([v for v in x['items'] if v['itemValue'].startswith('NanaType Probe')])}
                                    for x in a['registration_checks']],
            'resources': resources, 'lifecycle': life}


def main():
    p = argparse.ArgumentParser()
    p.add_argument('baseline', type=Path)
    p.add_argument('cold', type=Path)
    p.add_argument('regression', type=Path)
    p.add_argument('--output', type=Path, default=Path('docs/spout-cold-reconnect-validation.json'))
    args = p.parse_args()
    result = {'date': '2026-10-17', 'spout_revision': '3b9283a599cfcf40f600afcba0740b1e8deb6a1e',
              'nanaui_revision': '3f521953f0cc4d4d4d853ee93aaf235af56bf5c7',
              'note': 'OBS 60fps pixel measurements, approximately one-frame precision. Fence polling is an upper bound. SDK preparation publishes a separate transparent frame, without consuming producer fences.',
              **{key: summarize(root) for key, root in [('baseline', args.baseline), ('cold', args.cold), ('regression', args.regression)]}}
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
    for key in ['baseline', 'cold', 'regression']:
        r = result[key]
        print(key, json.dumps({'recovery': r['recovery'], 'cpu': r['lifecycle_cpu_ms'], 'events': r['lifecycle_counts'],
                              'fences': [r['native_sends'], r['native_releases'], r['native_pending']],
                              'expiry_ms': r['bridge_expiry_after_off_ms'], 'resources': r['resources']}, ensure_ascii=False))


if __name__ == '__main__':
    main()
