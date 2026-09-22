#!/bin/sh
# 모바일 PWA + Runner를 원격 호스트에 배포한다. (설계 0013 §9)
#
# 왜 스크립트인가: 빌드 호스트에 Node가 없어(worker) 번들은 개발 머신에서 만들어 보내야
# 하고, 소스는 private repo라 git bundle로 옮겨야 한다. 7단계를 손으로 하면 매번 한 단계를
# 빠뜨릴 여지가 생긴다 — 특히 "번들만 보내고 바이너리를 안 바꾸는" 실수는 증상이
# "왜 화면이 그대로지"로 나타나 원인을 찾기 어렵다.
#
# 사용:
#   deploy/push-mobile.sh [ssh-target] [branch]
#   deploy/push-mobile.sh worker feature2/JH2-remote-workspace-ux
set -eu

target="${1:-worker}"
branch="${2:-$(git rev-parse --abbrev-ref HEAD)}"
remote_src="${PRAXIS_REMOTE_SRC:-~/praxis-build}"
bundle_file="$(mktemp -u)/praxis-mobile.bundle"
mkdir -p "$(dirname "$bundle_file")"

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

# 커밋되지 않은 변경은 git bundle에 실리지 않는다. 소스와 배포 번들이 어긋나면
# 원격에서 재현 불가능한 상태가 되므로 여기서 멈춘다.
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "error: 커밋되지 않은 변경이 있습니다. 커밋 후 다시 실행하세요." >&2
  git status --short >&2
  exit 1
fi

echo "==> 모바일 번들 빌드"
npm run build:mobile

echo "==> git bundle 생성 ($branch)"
git bundle create "$bundle_file" "$branch"

echo "==> 전송 → $target"
scp -q "$bundle_file" "$target:~/praxis-mobile.bundle"
scp -q -r dist-mobile "$target:$remote_src/"
rm -f "$bundle_file"

echo "==> 원격 동기화 · 빌드 · 서비스 교체"
# 원격 브랜치명은 고정('mobile')으로 둔다 — 로컬 브랜치명이 바뀌어도 원격 체크아웃 절차가
# 흔들리지 않게 한다. fetch를 refs/heads로 직접 받으면 "checked out" 오류가 나므로
# remote ref를 경유한다.
ssh "$target" "set -eu
  cd $remote_src
  git fetch ~/praxis-mobile.bundle '$branch:refs/remotes/bundle/mobile'
  git checkout -f -B mobile refs/remotes/bundle/mobile
  . ~/.cargo/env
  cd src-tauri
  cargo build --release --bin praxis-runner
  systemctl --user stop praxis-runner
  cp target/release/praxis-runner ~/.local/bin/praxis-runner
  systemctl --user start praxis-runner
"

echo "==> 검증"
ssh "$target" "set -eu
  sleep 2
  systemctl --user is-active praxis-runner
  # 셸이 인증 밖에서 열리는지와 API가 여전히 인증 뒤인지를 함께 본다.
  printf '/m/       : '; curl -sS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:47831/m/
  printf 'v1(무인증): '; curl -sS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:47831/v1/health
"

echo
echo "완료. 폰에서 새로고침하세요."
echo "페어링 코드가 필요하면:"
echo "  ssh $target \"curl -sS -X POST -H \\\"Authorization: Bearer \\\$(cat ~/.config/praxis/pairing.token)\\\" http://127.0.0.1:47831/v1/mobile/pairings\""
