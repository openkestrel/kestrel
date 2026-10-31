"""Run only the recorded focused workloads; use a disposable baseline clone."""
import json
import pathlib
import shutil
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[3]
OUTPUT = pathlib.Path(__file__).resolve().parent
BEFORE = pathlib.Path('/private/tmp/kestrel-595-before')
TARGET = pathlib.Path('/private/tmp/kestrel-595-target')
TEST = 'a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance'
results = []


def run(name, source, args):
    command = ['mise', 'exec', '--', 'env', f'CARGO_TARGET_DIR={TARGET}',
               'CARGO_BUILD_JOBS=2', 'GIT_CONFIG_GLOBAL=/dev/null',
               'GIT_CONFIG_NOSYSTEM=1', 'cargo', *args,
               '--manifest-path', str(source / 'Cargo.toml')]
    if '--' in args:
        raise ValueError('Pass libtest arguments separately')
    if name.endswith(('first', 'warm', 'edit', 'prepare')):
        command += ['--', '--exact']
    started = time.time()
    with (OUTPUT / f'{name}.log').open('w') as log:
        result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
    row = dict(name=name, command=command, started_unix=started,
               wall_seconds=round(time.time() - started, 3), exit_code=result.returncode)
    results.append(row)
    (OUTPUT / 'local.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(row), flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)


if TARGET.exists():
    raise SystemExit(f'Remove the previous disposable target explicitly: {TARGET}')
for trial in range(1, 4):
    focused = ['test', '--locked', '-p', 'kestrel', '--test', 'suite', f'work::{TEST}']
    run(f'after-{trial}-build', ROOT, ['build', '--locked', '-p', 'kestrel'])
    run(f'after-{trial}-compile', ROOT, ['test', '--locked', '-p', 'kestrel', '--test', 'suite', '--no-run'])
    run(f'after-{trial}-first', ROOT, focused)
    run(f'after-{trial}-warm', ROOT, focused)
    source = ROOT / 'crates/kestrel/tests/suite/work.rs'
    original = source.read_bytes()
    try:
        source.write_bytes(original + b'\n')
        run(f'after-{trial}-edit', ROOT, focused)
    finally:
        source.write_bytes(original)
    shutil.rmtree(TARGET)

focused = ['test', '--locked', '-p', 'kestrel', '--test', 'work', TEST]
run('before-prepare', BEFORE, focused)
for trial in range(1, 4):
    run(f'before-{trial}-warm', BEFORE, focused)
    source = BEFORE / 'crates/kestrel/tests/work.rs'
    original = source.read_bytes()
    try:
        source.write_bytes(original + b'\n')
        run(f'before-{trial}-edit', BEFORE, focused)
    finally:
        source.write_bytes(original)
