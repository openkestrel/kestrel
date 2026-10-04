#!/usr/bin/env python3
import argparse
import codecs
import sys

import pyte
import errno
import fcntl
import json
import io
import tarfile
import os
import pty
import re
import select
import struct
import subprocess
import termios
import time
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('harness', choices=['claude', 'codex'])
parser.add_argument('--image', default='kestrel-dev-accept03')
parser.add_argument('--cols', type=int, default=240)
parser.add_argument('--term', default='xterm-256color')
parser.add_argument('--seconds', type=int, default=20)
parser.add_argument('--live', action='store_true')
parser.add_argument('--pipe', action='store_true')
parser.add_argument('--invalid-code', action='store_true')
args = parser.parse_args()
name = 'wayfinder-sign-in-' + uuid.uuid4().hex[:12]
command = ('/opt/acp/node_modules/@anthropic-ai/claude-agent-sdk-linux-arm64/claude setup-token'
           if args.harness == 'claude' else
           '/opt/acp/node_modules/.bin/codex login --device-auth -c cli_auth_credentials_store=\"file\"')
argv = ['docker', 'run', '--name', name, '-i']
if not args.pipe:
    argv += ['-t']
argv += ['--env', 'TERM=' + args.term, '--env', 'BROWSER=/bin/true',
         '--entrypoint', 'sh', args.image, '-c', command]
master = None
if args.pipe:
    process = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output_fd = process.stdout.fileno()
else:
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, args.cols, 0, 0))
    process = subprocess.Popen(argv, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
    os.close(slave)
    output_fd = master
raw = bytearray()
screen = pyte.Screen(args.cols, 40)
stream = pyte.Stream(screen)
decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')
snapshots = []
start = time.monotonic()
sent = False
announced = False
token_seen = False
credential_file = None
try:
    while time.monotonic() - start < args.seconds:
        ready = select.select([output_fd] + ([sys.stdin.fileno()] if args.live else []), [], [], 0.2)[0]
        if args.live and sys.stdin.fileno() in ready:
            answer = os.read(sys.stdin.fileno(), 4096).replace(b'\n', b'\r')
            if master is not None:
                os.write(master, answer)
            else:
                process.stdin.write(answer)
                process.stdin.flush()
        if output_fd in ready:
            try:
                chunk = os.read(output_fd, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            raw.extend(chunk)
            stream.feed(decoder.decode(chunk).replace('\n', '\r\n') if args.pipe else decoder.decode(chunk))
            view = '\n'.join(screen.display)
            if not snapshots or view != snapshots[-1]:
                snapshots.append(view)
                snapshots = snapshots[-2:]
            token_seen = token_seen or bool(re.search(r'sk-ant-oat[A-Za-z0-9_-]+', ''.join(line.strip() for line in screen.display)))
            if args.live and not announced:
                rows = screen.display
                url = ''
                for i, row in enumerate(rows):
                    if 'https://' in row:
                        url = row[row.index('https://'):].strip()
                        for following in rows[i+1:]:
                            if not following.strip():
                                break
                            url += following.strip()
                        break
                device = re.search(r'\b[A-Z0-9]{4,6}-[A-Z0-9]{4,6}\b', view)
                if url and ((args.harness == 'claude' and 'Paste code' in view) or device):
                    print(json.dumps({'harness': args.harness, 'url': url, 'code': device.group(0) if device else None}), flush=True)
                    announced = True
        if args.invalid_code and not sent and any('Paste code' in view for view in snapshots):
            payload = b'wayfinder-invalid-code\r'
            if master is not None:
                os.write(master, payload)
            else:
                process.stdin.write(payload)
                process.stdin.flush()
            sent = True
        if process.poll() is not None:
            break
finally:
    try:
        process.wait(timeout=0.5)
    except subprocess.TimeoutExpired:
        pass
    completed = process.poll() is not None
    if completed and args.harness == 'codex':
        inspection = subprocess.run(['docker', 'cp', name + ':/home/kestrel/.codex/auth.json', '-'], capture_output=True)
        if inspection.returncode == 0:
            with tarfile.open(fileobj=io.BytesIO(inspection.stdout)) as archive:
                auth = json.load(archive.extractfile(archive.getmembers()[0]))
            credential_file = {'keys': sorted(auth), 'auth_mode': auth.get('auth_mode'), 'token_keys': sorted(auth.get('tokens', {})), 'nonempty_tokens': bool(auth.get('tokens')) and all(bool(v) for v in auth.get('tokens', {}).values())}
    terminal = subprocess.run(['docker', 'exec', name, 'sh', '-c', 'stty -a < /dev/console'], capture_output=True, text=True)
    cleanup = subprocess.run(['docker', 'rm', '-f', name], capture_output=True)
    if process.poll() is None:
        process.terminate()
    process.wait(timeout=10)
    if master is not None:
        os.close(master)
plain = '\n'.join(snapshots[-2:])
plain = re.sub(r'\x1b\][^\x07]*(?:\x07|\x1b\\)', '', plain)
plain = re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', plain)
plain = plain.replace('\r', '')
plain = re.sub(r'https://.*?(?=\n\s*\n|$)', '<URL redacted>', plain, flags=re.S)
plain = '\n'.join(line.rstrip() for line in plain.splitlines())
plain = re.sub(r'sk-ant-[A-Za-z0-9_-]+', '<TOKEN redacted>', plain)
plain = re.sub(r'\b[A-Z0-9]{4,6}-[A-Z0-9]{4,6}\b', '<DEVICE CODE redacted>', plain)
print(json.dumps({'harness': args.harness, 'cols': args.cols, 'term': args.term,
                  'pipe': args.pipe, 'elapsed': round(time.monotonic()-start, 2),
                  'bytes': len(raw), 'exited_before_deadline': completed,
                  'exit_code': process.returncode, 'invalid_code_sent': sent,
                  'cleanup_ok': cleanup.returncode == 0, 'terminal': terminal.stdout, 'token_seen': token_seen, 'credential_file': credential_file, 'output': '[credential output suppressed]' if args.live or token_seen else plain}, indent=2))
