"""Isolated fixture tests for the exact embedded bootstrap source; no real service/SSH calls."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import tarfile
import tempfile
import types
import unittest
from unittest.mock import patch


SOURCE = (Path(__file__).resolve().parents[2] / 'deploy/bootstrap-runner.sh').read_text()
BOOTSTRAP = types.ModuleType('bootstrap_fixture')
exec(compile(SOURCE.split("<<'PY'\n", 1)[1].rsplit('\nPY', 1)[0], 'bootstrap-runner.sh', 'exec'), BOOTSTRAP.__dict__)


class BootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name).resolve()
        self.root = self.home / 'projects'
        self.root.mkdir()
        self.archive = self.home / 'runner.tar.gz'
        self.calls = []
        self.linger = False
        self.fail_linger = False
        self.write_archive()
        self.args = argparse.Namespace(archive=str(self.archive), root=str(self.root),
                                       sha256=BOOTSTRAP.sha_file(self.archive), operation='a' * 32,
                                       plan_digest='b' * 64)
        self.receipt = self.home / '.local/share/praxis/bootstrap-receipts' / ('a' * 32 + '.json')
        for mocked in [patch.dict(os.environ, {'HOME': str(self.home), 'XDG_CONFIG_HOME': str(self.home / '.config'),
                                               'XDG_DATA_HOME': str(self.home / '.local/share')}),
                       patch.object(BOOTSTRAP.platform, 'freedesktop_os_release', return_value={'ID': 'ubuntu', 'VERSION_ID': '24.04'}),
                       patch.object(BOOTSTRAP.platform, 'machine', return_value='x86_64'),
                       patch.object(BOOTSTRAP.shutil, 'which', side_effect=lambda name: '/fixture/' + name),
                       patch.object(BOOTSTRAP, 'command', side_effect=self.command)]:
            mocked.start()
            self.addCleanup(mocked.stop)
        self.umask = os.umask(0o077)
        self.addCleanup(os.umask, self.umask)

    def command(self, args, check=True):
        self.calls.append(args)
        if args[0] == 'loginctl' and args[1] == 'enable-linger':
            if self.fail_linger:
                raise RuntimeError('fixture permission denied')
            self.linger = True
        output = (b'yes\n' if self.linger else b'no\n') if args[0] == 'loginctl' and args[1] == 'show-user' else b''
        return types.SimpleNamespace(returncode=0, stdout=output)

    def write_archive(self, extra=None):
        with tarfile.open(self.archive, 'w:gz', format=tarfile.USTAR_FORMAT) as bundle:
            directory = tarfile.TarInfo('./')
            directory.type = tarfile.DIRTYPE
            bundle.addfile(directory)
            for name, data in [('praxis-runner', b'fixture binary; never executed'), ('VERSION', b'1.2.3'),
                               ('install-runner.sh', b'unsafe fixture installer; never executed'),
                               ('praxis-runner.example.toml', b''), ('praxis-runner.service', b'unsafe unit; ignored')]:
                entry = tarfile.TarInfo('./' + name)
                entry.size = len(data)
                bundle.addfile(entry, io.BytesIO(data))
            if extra:
                bundle.addfile(extra, io.BytesIO(b''))

    def test_installs_only_new_files_and_records_hashes_without_token(self):
        self.assertEqual(BOOTSTRAP.run(self.args)['phase'], 'ready')
        receipt = json.loads(self.receipt.read_text())
        self.assertEqual(receipt['phase'], 'service-ready')
        self.assertEqual(receipt['plan_digest'], self.args.plan_digest)
        self.assertEqual(receipt['linger_before'], 'no')
        self.assertEqual(len(receipt['files']), 4)
        for entry in receipt['files']:
            self.assertTrue(entry['created'])
            self.assertEqual(entry['sha256'], BOOTSTRAP.sha_file(Path(entry['path'])))
        config = (self.home / '.config/praxis/runner.toml').read_text()
        token = (self.home / '.config/praxis/pairing.token').read_text().strip()
        self.assertIn('execution_policy = "require_approval"', config)
        self.assertNotIn(token, self.receipt.read_text())
        self.assertEqual(stat.S_IMODE((self.home / '.config/praxis/pairing.token').stat().st_mode), 0o600)
        self.assertIn('PRAXIS_RUNNER_CONFIG=%h/.config/praxis/runner.toml', (self.home / '.config/systemd/user/praxis-runner.service').read_text())
        before = (self.home / '.local/bin/praxis-runner').read_bytes()
        with self.assertRaisesRegex(RuntimeError, 'already recorded'):
            BOOTSTRAP.run(self.args)
        self.assertEqual(before, (self.home / '.local/bin/praxis-runner').read_bytes())

    def test_existing_token_is_never_replaced(self):
        token = self.home / '.config/praxis/pairing.token'
        token.parent.mkdir(parents=True)
        token.write_text('existing-token')
        with self.assertRaisesRegex(RuntimeError, 'existing Runner'):
            BOOTSTRAP.run(self.args)
        self.assertEqual(token.read_text(), 'existing-token')
        self.assertFalse(self.receipt.exists())

    def test_archive_symlink_hardlink_traversal_and_duplicates_rejected(self):
        for name, kind in [('./VERSION', tarfile.REGTYPE), ('./evil', tarfile.SYMTYPE),
                           ('./praxis-runner.service', tarfile.LNKTYPE), ('../outside', tarfile.REGTYPE)]:
            with self.subTest(name=name, kind=kind):
                entry = tarfile.TarInfo(name)
                entry.type = kind
                entry.linkname = '/tmp/outside'
                self.write_archive(entry)
                with self.assertRaises(RuntimeError):
                    BOOTSTRAP.read_archive(self.archive, BOOTSTRAP.sha_file(self.archive))

    def test_digest_mismatch_creates_no_runner_files(self):
        self.args.sha256 = '0' * 64
        with self.assertRaisesRegex(RuntimeError, 'digest mismatch'):
            BOOTSTRAP.run(self.args)
        self.assertFalse(self.receipt.exists())
        self.assertFalse((self.home / '.config/praxis/pairing.token').exists())

    def test_symlink_parent_is_rejected(self):
        external = self.home / 'external'
        external.mkdir()
        (self.home / '.config').symlink_to(external)
        with self.assertRaisesRegex(RuntimeError, 'unsafe installation path'):
            BOOTSTRAP.run(self.args)
        self.assertEqual(list(external.iterdir()), [])

    def test_linger_failure_preserves_recovery_receipt_and_blocks_retry(self):
        self.fail_linger = True
        with self.assertRaisesRegex(RuntimeError, 'permission denied'):
            BOOTSTRAP.run(self.args)
        receipt = json.loads(self.receipt.read_text())
        self.assertEqual(receipt['phase'], 'needs-recovery')
        self.assertEqual(len(receipt['files']), 4)
        with self.assertRaisesRegex(RuntimeError, 'already recorded'):
            BOOTSTRAP.run(self.args)
        self.assertFalse(any('disable' in call or 'stop' in call for call in self.calls))

    def test_atomic_creation_refuses_existing_symlink(self):
        target = self.home / 'target'
        protected = self.home / 'protected'
        protected.write_text('preserve')
        target.symlink_to(protected)
        with self.assertRaises(FileExistsError):
            BOOTSTRAP.exclusive_file(target, b'replace', 0o600)
        self.assertEqual(protected.read_text(), 'preserve')

    def test_another_incomplete_receipt_blocks_install_even_without_target_files(self):
        self.receipt.parent.mkdir(parents=True, mode=0o700)
        previous = self.receipt.with_name('c' * 32 + '.json')
        previous.write_text(json.dumps({'phase': 'installing', 'files': []}))
        with self.assertRaisesRegex(RuntimeError, 'previous operation'):
            BOOTSTRAP.run(self.args)
        self.assertFalse((self.home / '.config/praxis/pairing.token').exists())
        self.assertEqual(self.calls, [])

    def test_root_and_operation_injection_fail_before_any_service_command(self):
        for root in [str(self.home), '/', str(self.root) + '\nmalicious']:
            self.args.root = root
            with self.assertRaises(RuntimeError):
                BOOTSTRAP.run(self.args)
        self.args.root = str(self.root)
        self.args.operation = '../escape'
        with self.assertRaisesRegex(RuntimeError, 'invalid operation'):
            BOOTSTRAP.run(self.args)
        self.assertEqual(self.calls, [])


if __name__ == '__main__':
    unittest.main()
