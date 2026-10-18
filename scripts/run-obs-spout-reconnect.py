"""Real OBS reconnect/expiry workload, using the isolated native Host probe."""
import argparse
import json
import runpy
import time
from pathlib import Path

import psutil

OBS = runpy.run_path(str(Path(__file__).with_name('obs-native-probe.py')))['OBS']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    parser.add_argument('--cold-only', action='store_true')
    parser.add_argument('--warm-rounds', type=int, default=10)
    parser.add_argument('--cold-rounds', type=int, default=3)
    parser.add_argument('--full-expiry', action='store_true')
    parser.add_argument('--full-expiry-rounds', type=int, default=1)
    args = parser.parse_args()
    root = args.root
    obs = OBS(root.parent)
    app = psutil.Process(int((root / 'app.pid').read_text()))
    obsproc = psutil.Process(int((root.parent / 'obs.pid').read_text()))
    deadline = time.monotonic() + 60
    while True:
        try:
            if json.loads((root / 'live.json').read_text()).get('sends', 0) >= 2:
                break
        except (ValueError, FileNotFoundError):
            pass
        if not app.is_running() or time.monotonic() >= deadline:
            raise RuntimeError('native sender did not become ready')
        time.sleep(.1)

    actions, samples, checks = [], [], []
    start = time.monotonic()
    last_sample = -1
    seq = 100

    def save():
        (root / 'timeline.json').write_text(json.dumps(actions, ensure_ascii=False, indent=2))
        (root / 'samples.json').write_text(json.dumps(samples))

    def wait(seconds):
        nonlocal last_sample
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            now = time.monotonic() - start
            if now - last_sample >= 1:
                last_sample = now
                row = {'t': now, 'app_private': app.memory_info().private,
                       'app_handles': app.num_handles(), 'app_threads': app.num_threads(),
                       'obs_private': obsproc.memory_info().private,
                       'obs_handles': obsproc.num_handles(), 'obs': obs.call('GetStats')}
                try:
                    row['native'] = json.loads((root / 'live.json').read_text())
                except (ValueError, FileNotFoundError):
                    pass
                samples.append(row)
                save()
            time.sleep(.05)

    def command(op, **values):
        nonlocal seq
        seq += 1
        value = {'op': op, **values}
        path = root / 'commands' / f'{seq:05d}.tmp'
        path.write_text(json.dumps(value, ensure_ascii=False))
        path.rename(path.with_suffix('.json'))
        actions.append({'t': time.monotonic() - start, 'command': value})
        save()
        print(value, flush=True)

    def marker():
        for color in [0xffb04020, 0xff303030]:
            before = time.monotonic() - start
            obs.call('SetInputSettings', {'inputName': 'ProbeBackground', 'inputSettings': {'color': color}})
            actions.append({'t': before, 'marker': color, 'returned': time.monotonic() - start})
            save()
            wait(.2)

    def senders(label):
        values = obs.call('GetInputPropertiesListPropertyItems',
                          {'inputName': 'Display', 'propertyName': 'spoutsenders'})
        checks.append({'t': time.monotonic() - start, 'label': label, 'items': values['propertyItems']})
        (root / 'registration-checks.json').write_text(json.dumps(checks, ensure_ascii=False, indent=2))

    obs.call('SetRecordDirectory', {'recordDirectory': str(root.parent / 'recordings')})
    before = obs.call('GetStats')
    obs.call('StartRecord')
    wait(1)
    marker()
    for text in ['OBS 快速重开：保留发送器和共享围栏', '第二条历史：透明帧不影响原生窗口']:
        command('send', text='[@name=NATIVE-PROBE]' + text)
        wait(.2)
        command('skip')
        wait(1)
    if not args.cold_only:
        for hidden in [False, True]:
            command('all_visible', visible=not hidden)
            wait(1)
            for i in range(args.warm_rounds):
                command('outputs', enabled=False)
                wait(.4)
                if i == 0:
                    senders('warm-hidden' if hidden else 'warm-visible')
                command('outputs', enabled=True)
                wait(1)
                if i % 3 == 0 or args.warm_rounds < 10:
                    command('skin', id=['nanatype.forest', 'nanatype.tidal', 'nanatype.lantern', 'nanatype.citrus'][min(3, i * 4 // max(1, args.warm_rounds))])
                    wait(.4)
        command('skin', id='user.nativeprobe')
        wait(2)
        source = Path('crates/theme/builtin/tidal/artwork.wgsl').read_text()
        effect = root / 'app/data/packages/user.nativeprobe/effect.wgsl'
        for i in range(4):
            command('outputs', enabled=False)
            effect.write_text('fn effect(i:EffectInput)->vec4<f32>{loop{} return vec4(0.);}'
                              if i == 1 else source + f'\n// reconnect reload {i}\n')
            actions.append({'t': time.monotonic() - start, 'hot_reload': i, 'valid': i != 1})
            save()
            wait(.7)
            command('outputs', enabled=True)
            wait(1.2)
    for i in range(args.cold_rounds):
        command('outputs', enabled=False)
        wait(1)
        senders(f'grace-{i}')
        wait(5)
        senders(f'expired-{i}')
        command('outputs', enabled=True)
        wait(2)
    for i in range(args.full_expiry_rounds if args.full_expiry else 0):
        command('outputs', enabled=False)
        wait(32)
        senders(f'expired-full-{i}')
        command('outputs', enabled=True)
        wait(2)
    marker()
    obs.screenshot('Probe', root / 'final.png')
    after = obs.call('GetStats')
    record = obs.call('StopRecord')
    (root / 'record-result.json').write_text(json.dumps({'before': before, 'after': after, 'record': record}))
    command('outputs', enabled=False)
    wait(35 if args.full_expiry else 15)
    senders('final-expired')
    command('quit')
    obs.ws.close()


if __name__ == '__main__':
    main()
