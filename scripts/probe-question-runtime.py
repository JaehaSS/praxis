#!/usr/bin/env python3
"""Explicit, foreground live Codex protocol probe using the production Rust adapter.
Uses the existing Codex account, a disposable cwd and an in-memory MCP dispatcher.
Only the probe process disables unrelated configured MCP servers/apps; no config is edited.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

root = Path(__file__).resolve().parent.parent
binary = shutil.which('codex')
if not binary:
    raise SystemExit('Codex not found')
config_path = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex'))) / 'config.toml'
config = tomllib.loads(config_path.read_text()) if config_path.exists() else {}
overrides = ['-c', 'features.apps=false']
for name in config.get('mcp_servers', {}):
    if name == 'praxis_preview':
        continue
    if not all(c.isalnum() or c in '_-' for c in name):
        raise SystemExit('Unsupported MCP config key; refusing an ambiguous override')
    overrides += ['-c', f'mcp_servers.{name}.enabled=false']
with tempfile.TemporaryDirectory(prefix='praxis-live-adapter-') as temp:
    wrapper = Path(temp) / 'codex-probe'
    wrapper.write_text(f'#!{sys.executable}\nimport os,sys\nos.execv({binary!r}, [{binary!r}] + sys.argv[1:] + {overrides!r})\n')
    wrapper.chmod(0o755)
    env = dict(os.environ)
    env['PRAXIS_QUESTION_LIVE_CODEX'] = str(wrapper)
    env['PRAXIS_QUESTION_LIVE_EVIDENCE'] = str(root / 'docs/plans/evidence/agent-session-live-adapter.json')
    result = subprocess.run(['cargo', 'test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'convo_question_runtime_test', 'live_codex_question_resume_and_fresh_mcp_lease', '-j', '2', '--', '--ignored', '--exact', '--test-threads=1'], cwd=root, env=env)
    raise SystemExit(result.returncode)
