#!/usr/bin/env bash
#
# rust-analyzer를 MCP(mcp-language-server)로 에이전트에 물릴 때 쓰는 런처.
#
# 기본 `.mcp.json`에는 들어 있지 않다. 세션(worktree)마다 rust-analyzer가 한 벌씩 뜨는데
# 이 프로젝트 규모에서 인스턴스당 2~6GB를 쓴다 — 세션 5개면 24GB 머신이 통째로 스왑으로
# 밀려 load 46, sys 59%가 된다. 실제로 그렇게 됐다. 필요한 세션에서만 켠다.
#
#   claude mcp add lsp-rust -- bash scripts/lsp-rust-mcp.sh
#
# 켜면 따라오는 것 하나 — check 전용 target을 빌드 target에서 떼어낸다.
# rust-analyzer는 파일이 바뀔 때마다 `cargo check --workspace`를 돌린다. 그것이
# ~/.cargo/config.toml의 build.target-dir(= 모든 worktree가 공유하는 shared-target)을
# 그대로 쓰면 사람이 돌리는 `cargo build`와 같은 .cargo-lock을 두고 경합한다.
# 실측에서 cargo 세 개가 1분 44초씩 서로를 기다리고 있었다.
#
# CARGO_TARGET_DIR은 config 파일의 build.target-dir보다 우선한다(실측 확인).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# 이미 지정돼 있으면 존중한다 — 호출자가 target을 따로 두고 싶을 수 있다.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/src-tauri/target/rust-analyzer}"

exec "$(command -v mcp-language-server || echo "$HOME/go/bin/mcp-language-server")" \
  --workspace "$root/src-tauri" --lsp rust-analyzer
