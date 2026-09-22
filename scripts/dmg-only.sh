#!/usr/bin/env bash
#
# 이미 만들어진 .app을 dmg로만 다시 포장한다.
#
# `npm run tauri build`가 프론트(vite) → cargo → .app → dmg를 순서대로 도는 것과 달리
# 앞의 셋을 전부 건너뛰고 마지막 단계만 실행한다. .app이 이미 있는데 dmg 단계만 실패했을 때
# (CLAUDE.md "dmg 번들링이 실패할 때"), 또는 배포용 dmg만 다시 뽑고 싶을 때 쓴다.
#
# Tauri가 쓰는 것과 같은 bundle_dmg.sh를 같은 인자로 호출하므로 결과물은 동등하다.
# 인자 기본값은 tauri-bundler의 DmgConfig 기본값이다 — 창 660x400, 앱 아이콘 (180,170),
# Applications 링크 (480,170).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONF="$ROOT/src-tauri/tauri.conf.json"

# target/ 위치는 하드코딩할 수 없다. `CARGO_TARGET_DIR`나 `~/.cargo/config.toml`의
# `build.target-dir`로 옮겨져 있을 수 있고, 이 저장소는 실제로 worktree마다 target이
# 통째로 복제되는 것을 막으려 공유 target을 쓴다. 그래서 cargo에게 직접 묻는다 —
# 해석 규칙(환경변수 · 설정 파일 계층)을 여기서 흉내 내면 반드시 어긋난다.
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 --manifest-path "$ROOT/src-tauri/Cargo.toml" 2>/dev/null \
	| node -e 'let s="";process.stdin.on("data",c=>s+=c);process.stdin.on("end",()=>{let d="";try{d=JSON.parse(s).target_directory||""}catch{}console.log(d)})' 2>/dev/null || true)"
# cargo가 없거나 실패하면 기본 위치로 돌아간다 — 아래 존재 검사가 어차피 막아 준다.
[ -n "$TARGET_DIR" ] || TARGET_DIR="$ROOT/src-tauri/target"

BUNDLE="$TARGET_DIR/release/bundle"
DMG_DIR="$BUNDLE/dmg"
BUNDLER="$DMG_DIR/bundle_dmg.sh"

PRODUCT="$(node -p "require('$CONF').productName")"
VERSION="$(node -p "require('$CONF').version")"
APP="$BUNDLE/macos/$PRODUCT.app"

case "$(uname -m)" in
	arm64|aarch64) ARCH=aarch64 ;;
	x86_64)        ARCH=x86_64 ;;
	*)             ARCH="$(uname -m)" ;;
esac
OUT="${PRODUCT}_${VERSION}_${ARCH}.dmg"

# .app과 번들러 스크립트는 둘 다 빌드 산출물이다. 없으면 전체 빌드가 한 번은 돌아야 한다.
if [ ! -d "$APP" ]; then
	echo "✗ .app이 없습니다: $APP" >&2
	echo "  먼저 'npm run tauri build'를 한 번 돌리세요. 이 스크립트는 포장만 다시 합니다." >&2
	exit 1
fi
if [ ! -x "$BUNDLER" ]; then
	echo "✗ bundle_dmg.sh가 없습니다: $BUNDLER" >&2
	echo "  Tauri가 빌드 때 추출하는 파일입니다. 'npm run tauri build'를 한 번 돌리세요." >&2
	exit 1
fi

# 남은 마운트가 dmg 단계를 막는 것이 이 프로젝트의 알려진 실패 원인이다.
# 우리 번들 디렉터리를 백킹으로 하는 것만 골라 떼어낸다 — 무관한 dmg는 건드리지 않는다.
detached=0
while read -r dev; do
	[ -n "$dev" ] || continue
	hdiutil detach "$dev" -quiet 2>/dev/null || hdiutil detach "$dev" -force -quiet 2>/dev/null || true
	detached=$((detached + 1))
done < <(hdiutil info 2>/dev/null | awk -v root="$BUNDLE" '
	/^image-path/ { keep = (index($0, root) > 0) }
	keep && /^\/dev\/disk/ { d = $1; sub(/s[0-9]+$/, "", d); print d }
' | sort -u)
[ "$detached" -gt 0 ] && echo "· 남아 있던 마운트 ${detached}건 해제"

# 실패한 이전 실행이 남긴 임시 이미지(각 100MB대)도 치운다.
find "$BUNDLE" -name "rw.*.dmg" -delete 2>/dev/null || true

# bundle_dmg.sh는 <source_folder>의 내용물을 통째로 담는다. macos/ 를 그대로 주면
# 거기 굴러다니는 .DS_Store까지 들어가 창 레이아웃을 덮어쓸 수 있어, .app만 담은 임시 폴더를 쓴다.
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/$PRODUCT.app"

ARGS=(
	--volname "$PRODUCT"
	--icon "$PRODUCT.app" 180 170
	--app-drop-link 480 170
	--window-size 660 400
)
ICON="$DMG_DIR/icon.icns"
[ -f "$ICON" ] || ICON="$APP/Contents/Resources/icon.icns"
[ -f "$ICON" ] && ARGS+=(--volicon "$ICON")

rm -f "$DMG_DIR/$OUT"
echo "· 포장: $PRODUCT $VERSION ($ARCH) ← $APP"
( cd "$DMG_DIR" && "$BUNDLER" "${ARGS[@]}" "$OUT" "$STAGE" )

[ -f "$DMG_DIR/$OUT" ] || { echo "✗ dmg가 생성되지 않았습니다" >&2; exit 1; }
echo "✓ $DMG_DIR/$OUT ($(du -h "$DMG_DIR/$OUT" | cut -f1))"
