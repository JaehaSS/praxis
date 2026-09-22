#!/usr/bin/env bash
# New installations only. No apt/sudo, automatic replacement, or guessed rollback.
set -euo pipefail
exec python3 - "$@" <<'PY'
import argparse, fcntl, hashlib, json, os, pathlib, platform, re, secrets, shutil, stat, subprocess, sys, tarfile, tempfile

MAX_ARCHIVE = 512 * 1024 * 1024
MAX_EXPANDED = 1024 * 1024 * 1024
SERVICE = '''[Unit]
Description=Praxis headless runner
After=network-online.target
StartLimitIntervalSec=0
[Service]
Type=simple
Environment=PRAXIS_RUNNER_CONFIG=%h/.config/praxis/runner.toml
Environment=PRAXIS_RUNNER_DB=%h/.local/share/praxis/runner.sqlite
Environment=PATH=%h/.local/bin:%h/.cargo/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=%h/.local/bin/praxis-runner
Restart=always
RestartSec=5
KillMode=control-group
NoNewPrivileges=true
PrivateTmp=true
UMask=0077
[Install]
WantedBy=default.target
'''

def require(condition, message):
    if not condition:
        raise RuntimeError(message)

def safe_path(path, home, missing=False):
    """All existing components below HOME must be owned, nonlinks, and not writable by others."""
    path = pathlib.Path(path)
    require(path.is_absolute() and path.is_relative_to(home), 'unsafe installation path')
    current = home
    for part in [None] + list(path.relative_to(home).parts):
        if part is not None:
            current = current / part
        try:
            info = current.lstat()
        except FileNotFoundError:
            require(missing, 'missing installation path')
            continue
        require(not stat.S_ISLNK(info.st_mode) and info.st_uid == os.getuid()
                and not info.st_mode & 0o022, 'unsafe installation path')
    return path

def directory(path, home):
    safe_path(path, home, missing=True)
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    safe_path(path, home)
    require(path.is_dir(), 'installation directory is not a directory')

def sha_file(path):
    with path.open('rb') as stream:
        digest = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
        return digest.hexdigest()

def command(args, check=True):
    result = subprocess.run(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, timeout=20, check=False)
    require(not check or result.returncode == 0, 'user service preparation failed; inspect receipt')
    return result

def exclusive_file(path, content, mode):
    """Atomically commit a new file; a preexisting file or link always wins."""
    descriptor, temporary = tempfile.mkstemp(prefix='.praxis-new-', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
            os.fchmod(stream.fileno(), mode)
        os.link(temporary, path, follow_symlinks=False)
    finally:
        os.unlink(temporary)

def write_receipt(path, receipt):
    descriptor, temporary = tempfile.mkstemp(prefix='.receipt-', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'w') as stream:
            json.dump(receipt, stream, ensure_ascii=False)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)

def read_archive(archive, expected_digest):
    require(archive.is_file() and not archive.is_symlink() and archive.stat().st_size <= MAX_ARCHIVE, 'unsafe archive')
    require(sha_file(archive) == expected_digest, 'archive digest mismatch')
    allowed = {'praxis-runner', 'VERSION', 'install-runner.sh', 'praxis-runner.example.toml', 'praxis-runner.service'}
    with tarfile.open(archive, 'r:gz') as bundle:
        seen, total = set(), 0
        for member in bundle:
            name = member.name
            require(name not in seen, 'duplicate archive entry')
            seen.add(name)
            if name in ('.', './'):
                require(member.isdir(), 'unsafe archive directory')
                continue
            require(name.startswith('./') and name[2:] in allowed and member.isfile(), 'unsafe archive entry')
            require(not member.pax_headers, 'unsupported archive metadata')
            total += member.size
            require(0 <= member.size <= MAX_EXPANDED and total <= MAX_EXPANDED, 'archive too large')
        require('./praxis-runner' in seen and './VERSION' in seen, 'incomplete archive')
        # Read only binary bytes. Never extract paths or execute archive-supplied scripts.
        return bundle.extractfile('./praxis-runner').read()

def run(args):
    os.umask(0o077)
    require(re.fullmatch('[a-f0-9]{32}', args.operation) is not None, 'invalid operation')
    require(re.fullmatch('[a-f0-9]{64}', args.sha256) is not None
            and re.fullmatch('[a-f0-9]{64}', args.plan_digest) is not None, 'invalid digest')
    release = platform.freedesktop_os_release()
    require(release.get('ID') == 'ubuntu' and release.get('VERSION_ID') == '24.04'
            and platform.machine() == 'x86_64', 'unsupported server')
    require(shutil.which('systemctl') and shutil.which('loginctl'), 'missing user service tools')
    home = pathlib.Path.home()
    require(home.is_absolute() and home.resolve() == home, 'unsafe home')
    safe_path(home, home)
    require(os.environ.get('XDG_CONFIG_HOME', str(home / '.config')) == str(home / '.config')
            and os.environ.get('XDG_DATA_HOME', str(home / '.local/share')) == str(home / '.local/share'),
            'nonstandard XDG layout requires manual preparation')
    root = pathlib.Path(args.root)
    require(root.is_absolute() and root.is_dir() and str(root.resolve()) == args.root
            and root not in (pathlib.Path('/'), home) and os.access(root, os.R_OK | os.W_OK | os.X_OK),
            'invalid canonical workspace root')
    # Existing Runner config parser is narrower than full TOML; do not guess escaping.
    require(not any(c in str(root) + str(home) for c in '\\"\r\n\t,[]')
            and not any(ord(c) < 32 for c in str(root) + str(home)), 'unsupported configuration path')
    config, data = home / '.config/praxis', home / '.local/share/praxis'
    binary, unit = home / '.local/bin/praxis-runner', home / '.config/systemd/user/praxis-runner.service'
    token, configuration = config / 'pairing.token', config / 'runner.toml'
    targets = [token, configuration, binary, unit]
    for target in targets + [data / 'runner.sqlite']:
        safe_path(target, home, missing=True)
    directory(data, home)
    lock_path = data / 'bootstrap.lock'
    safe_path(lock_path, home, missing=True)
    lock = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        receipts = data / 'bootstrap-receipts'
        directory(receipts, home)
        require(stat.S_IMODE(receipts.stat().st_mode) == 0o700, 'unsafe receipt directory')
        receipt_path = receipts / (args.operation + '.json')
        safe_path(receipt_path, home, missing=True)
        require(not receipt_path.exists(), 'operation already recorded; re-probe instead of reinstalling')
        # A previous attempt can fail after recording intent but before creating any
        # target. An empty target set is not permission for another operation.
        require(not any(receipts.iterdir()), 'previous operation requires manual recovery')
        require(not any(path.exists() for path in targets + [data / 'runner.sqlite']), 'existing Runner requires reuse')
        fragment = command(['systemctl', '--user', 'show', 'praxis-runner.service', '--property=FragmentPath', '--value'], False)
        require(not fragment.stdout.strip(), 'existing Runner service requires reuse')
        command(['systemctl', '--user', 'show-environment'])
        linger = command(['loginctl', 'show-user', str(os.getuid()), '--property=Linger', '--value'])
        require(linger.stdout.strip() in (b'yes', b'no'), 'cannot inspect linger state')
        binary_bytes = read_archive(pathlib.Path(args.archive), args.sha256)
        for target in targets:
            directory(target.parent, home)
        receipt = {'schema_version': 1, 'operation': args.operation, 'plan_digest': args.plan_digest,
                   'archive_sha256': args.sha256, 'uid': os.getuid(), 'phase': 'installing',
                   'linger_before': linger.stdout.strip().decode(), 'service_before': 'absent', 'files': []}
        exclusive_file(receipt_path, json.dumps(receipt).encode(), 0o600)
        try:
            token_bytes = (secrets.token_hex(32) + '\n').encode()
            config_bytes = ('bind = "127.0.0.1:47831"\nrepository_roots = ["' + str(root)
                            + '"]\nexecution_policy = "require_approval"\npairing_token_file = "'
                            + str(token) + '"\n').encode()
            for target, content, mode in [(token, token_bytes, 0o600), (configuration, config_bytes, 0o600),
                                          (binary, binary_bytes, 0o755), (unit, SERVICE.encode(), 0o644)]:
                safe_path(target, home, missing=True)
                receipt['files'].append({'path': str(target), 'created': False, 'sha256': hashlib.sha256(content).hexdigest()})
                write_receipt(receipt_path, receipt)
                exclusive_file(target, content, mode)
                receipt['files'][-1]['created'] = True
                write_receipt(receipt_path, receipt)
            receipt['phase'] = 'files-created'
            write_receipt(receipt_path, receipt)
            command(['systemctl', '--user', 'daemon-reload'])
            command(['systemctl', '--user', 'enable', '--now', 'praxis-runner.service'])
            if receipt['linger_before'] != 'yes':
                command(['loginctl', 'enable-linger', str(os.getuid())])
            command(['systemctl', '--user', 'is-active', '--quiet', 'praxis-runner.service'])
            require(command(['loginctl', 'show-user', str(os.getuid()), '--property=Linger', '--value']).stdout.strip() == b'yes',
                    'linger remains disabled; inspect receipt')
            receipt['phase'] = 'service-ready'
            write_receipt(receipt_path, receipt)
        except Exception:
            receipt['phase'] = 'needs-recovery'
            write_receipt(receipt_path, receipt)
            raise
        return {'phase': 'ready', 'receipt': str(receipt_path)}
    finally:
        os.close(lock)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for name in ['archive', 'root', 'sha256', 'operation', 'plan-digest']:
        parser.add_argument('--' + name, required=True)
    try:
        print(json.dumps(run(parser.parse_args())))
    except Exception:
        print('Runner preparation failed; reconnect and inspect the operation receipt before retrying.', file=sys.stderr)
        sys.exit(1)
PY
