"""Match real OBS pixel recovery to commands and production fence records."""
import argparse
import json
import statistics
import subprocess
from pathlib import Path

import imageio_ffmpeg
import numpy as np


def distribution(values):
    values = sorted(values)
    return {'n': len(values), 'median_ms': statistics.median(values) if values else None,
            'p95_ms': values[int(len(values) * .95)] if values else None,
            'max_ms': max(values) if values else None}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    args = parser.parse_args()
    root = args.root
    timeline = json.loads((root / 'timeline.json').read_text())
    native = json.loads((root / 'native-report.json').read_text())
    record = json.loads((root / 'record-result.json').read_text())
    video = record['record']['outputPath']
    pipe = subprocess.Popen([imageio_ffmpeg.get_ffmpeg_exe(), '-v', 'error', '-i', video,
                             '-an', '-vf', 'scale=480:270', '-f', 'rawvideo', '-pix_fmt',
                             'rgb24', 'pipe:1'], stdout=subprocess.PIPE)
    presence = {'display': [], 'history': []}
    regions = {'display': (slice(5, 85), slice(10, 250)),
               'history': (slice(105, 195), slice(10, 250))}
    pulses = []
    blue_before = False
    count = 0
    while True:
        data = pipe.stdout.read(480 * 270 * 3)
        if not data:
            break
        assert len(data) == 480 * 270 * 3
        pixels = np.frombuffer(data, dtype=np.uint8).reshape(270, 480, 3).astype(np.int16)
        bg = pixels[250, 450]
        blue = bool(bg[2] - bg[0] > 60)
        if blue and not blue_before:
            pulses.append(count / 60)
        blue_before = blue
        for name, region in regions.items():
            presence[name].append(bool(np.count_nonzero(np.max(abs(pixels[region] - bg), axis=2) > 10) >= 50))
        count += 1
    assert pipe.wait() == 0
    marker_actions = [a['t'] for a in timeline if a.get('marker') == 0xffb04020]
    assert len(marker_actions) == len(pulses) == 2, (marker_actions, pulses)
    offsets = [a - p for a, p in zip(marker_actions, pulses)]
    offset = statistics.median(offsets)
    commands = [a for a in timeline if 'command' in a]
    native_actions = native['summary']['actions']
    assert len(commands) == len(native_actions)
    for a, b in zip(commands, native_actions):
        assert a['command'] == b['command']
        a['native_ms'] = b['ms']
    intervals = []
    off = None
    for a in commands:
        c = a['command']
        if c['op'] != 'outputs':
            continue
        if not c['enabled']:
            off = a
        elif off:
            intervals.append((off, a))
            off = None
    details = []
    for off, on in intervals:
        row = {'off_s': off['t'], 'on_s': on['t'], 'off_duration_s': on['t'] - off['t'], 'warm': on['t'] - off['t'] < 5,
               'outputs': {}}
        next_off = next((a['t'] for a in commands if a['t'] > on['t'] and
                         a['command'] == {'op': 'outputs', 'enabled': False}), count / 60 + offset)
        for name, window in [('display', 2), ('history', 3)]:
            first = next((i for i in range(max(0, int((on['t'] - offset) * 60)), count)
                          if i / 60 + offset >= on['t'] and presence[name][i]), None)
            assert first is not None and first / 60 + offset < next_off
            end = min(count, int((next_off - offset) * 60))
            later_blanks = sum(not x for x in presence[name][first:end])
            before = [s for s in native['sends'] if s['window'] == window and s['ms'] < off['native_ms']]
            after = next(s for s in native['sends'] if s['window'] == window and
                         s['ms'] >= on['native_ms'] and not s['suspended'])
            clear = [s for s in native['sends'] if s['window'] == window and
                     off['native_ms'] <= s['ms'] < on['native_ms'] and s['suspended']]
            has_off_blank = any(not presence[name][i] for i in range(
                max(0, int((off['t'] - offset + .15) * 60)), min(count, int((on['t'] - offset) * 60))))
            row['outputs'][name] = {'obs_recovery_ms': max(0, (first / 60 + offset - on['t']) * 1000),
                                    'native_recovery_ms': after['ms'] - on['native_ms'],
                                    'same_pool': before[-1]['generation'] == after['generation'],
                                    'suspended_sends': len(clear), 'off_became_blank': has_off_blank,
                                    'later_blank_frames': later_blanks}
        details.append(row)
    results = {}
    for warm in [True, False]:
        results['warm' if warm else 'cold'] = {name: {
            'obs': distribution([r['outputs'][name]['obs_recovery_ms'] for r in details if r['warm'] == warm]),
            'native': distribution([r['outputs'][name]['native_recovery_ms'] for r in details if r['warm'] == warm]),
            'all_same_pool': all(r['outputs'][name]['same_pool'] for r in details if r['warm'] == warm),
        } for name in presence}
    for group, predicate in [('cold_bridge', lambda r: 5 <= r['off_duration_s'] < 30),
                             ('cold_expired', lambda r: r['off_duration_s'] >= 30)]:
        results[group] = {name: {
            'obs': distribution([r['outputs'][name]['obs_recovery_ms'] for r in details if predicate(r)]),
            'native': distribution([r['outputs'][name]['native_recovery_ms'] for r in details if predicate(r)]),
        } for name in presence}
    summary = {'video': video, 'decoded_frames': count, 'video_offset_s': offset,
               'marker_offsets_s': offsets, 'results': results, 'intervals': details,
               'native': native['summary'],
               'obs_render_skipped_delta': record['after']['renderSkippedFrames'] - record['before']['renderSkippedFrames'],
               'obs_output_skipped': record['after']['outputSkippedFrames'],
               'registration_checks': json.loads((root / 'registration-checks.json').read_text()),
               'note': 'Pixel recovery is sampled at 60 fps, aligned by two recorded background markers. Fence polling is an upper bound, not a GPU timestamp.'}
    samples = json.loads((root / 'samples.json').read_text())
    cold_start = next(r['off_s'] for r in details if not r['warm'])
    warm_end = min(samples, key=lambda s: abs(s['t'] - (cold_start + 2)))['obs']
    summary['warm_obs_render_skipped_delta'] = warm_end['renderSkippedFrames'] - record['before']['renderSkippedFrames']
    summary['warm_obs_output_skipped'] = warm_end['outputSkippedFrames']
    (root / 'reconnect-analysis.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2, allow_nan=False))
    for check in summary['registration_checks']:
        senders = [x for x in check['items'] if x['itemValue'].startswith('NanaType Probe')]
        assert len(senders) == (0 if 'expired' in check['label'] else 2), check
    for row in details:
        for value in row['outputs'].values():
            assert value['suspended_sends'] == 1 and value['off_became_blank'], row
            assert value['later_blank_frames'] == 0, row
            if row['warm']:
                assert value['same_pool'], row
            else:
                assert not value['same_pool'], row
    assert native['summary']['pending'] == 0 and native['summary']['sends'] == native['summary']['retired']
    assert not native['summary']['errors']
    print(json.dumps({k: summary[k] for k in ['decoded_frames', 'video_offset_s', 'results', 'obs_render_skipped_delta', 'obs_output_skipped']}, indent=2))


if __name__ == '__main__':
    main()
