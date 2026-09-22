#!/usr/bin/env python3
"""Explicit, foreground live Claude MCP probe using the production Rust adapter.

Settles the one thing the offline suite cannot: whether the Claude CLI actually holds an
`mcp__praxis-preview__ask_user` call open past its default tool timeout. The probe answers
later than that default on purpose — an answer that still reaches the turn is the evidence.

Uses the existing Claude account, a disposable cwd and an in-memory MCP dispatcher that is
never reached. Nothing in ~/.claude is edited.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parent.parent
binary = shutil.which('claude')
if not binary:
    raise SystemExit('Claude CLI not found')
delay = os.environ.get('PRAXIS_QUESTION_LIVE_DELAY_SECS', '120')
if int(delay) <= 90:
    raise SystemExit('Delay must exceed the 90s default tool timeout to prove anything')
env = dict(os.environ)
env['PRAXIS_QUESTION_LIVE_CLAUDE'] = binary
env['PRAXIS_QUESTION_LIVE_DELAY_SECS'] = delay
env['PRAXIS_QUESTION_LIVE_EVIDENCE'] = str(
    root / 'docs/plans/evidence/claude-question-mcp-live.json'
)
Path(env['PRAXIS_QUESTION_LIVE_EVIDENCE']).parent.mkdir(parents=True, exist_ok=True)
print(f'Holding the question open for {delay}s; the turn should survive it.', file=sys.stderr)
result = subprocess.run(
    [
        'cargo', 'test', '--manifest-path', 'src-tauri/Cargo.toml',
        '--test', 'convo_question_local_test',
        'live_claude_blocks_on_ask_user_past_the_default_tool_timeout',
        '-j', '2', '--', '--ignored', '--exact', '--test-threads=1', '--nocapture',
    ],
    cwd=root,
    env=env,
)
raise SystemExit(result.returncode)
