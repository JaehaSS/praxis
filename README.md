# Praxis

**한국어** · [English](README.en.md)

**AI 코딩 에이전트를 여러 개 동시에 돌리고, 바뀐 내용을 직접 확인한 뒤 승인하는 데스크톱 IDE입니다.**

에이전트에게 일을 시키는 것은 터미널에서도 할 수 있습니다. Praxis는 그다음 단계를 맡습니다.
작업마다 **격리된 git worktree**를 만들고, 에이전트가 그 안에서만 파일을 고치게 하고, 작업이
끝나면 diff를 보여 준 뒤 **승인할지 폐기할지 사람이 결정하게** 합니다. 승인하기 전까지 원본
브랜치는 바뀌지 않습니다.

<!-- 스크린샷 자리: 홈 화면 1장 + 검토/diff 화면 1장. 아직 없음 — 캡처해서 docs/assets/에 넣고 여기 링크할 것. -->

```
지시문  →  격리 worktree 생성  →  에이전트 실행  →  diff 검토  →  승인 또는 폐기
           praxis/<작업명> 브랜치                     (부분 적용 가능)
```

## 무엇이 다른가

- **원본을 건드리지 않습니다.** 작업마다 `.praxis/worktrees/` 아래에 worktree와 전용 브랜치를
  만듭니다. 폐기하면 worktree가 통째로 사라지고, 승인해야 커밋과 머지가 일어납니다.
- **여러 작업을 한 번에 돌립니다.** 기본 8개(설정에서 1~64)를 각자의 worktree와 터미널 세션에서
  병렬로 실행합니다.
- **벤더를 섞어 씁니다.** Claude Code · Codex · Antigravity(Gemini)를 작업마다 고를 수 있고, 같은
  지시문을 여러 에이전트에게 맡겨 결과를 비교하거나 서로의 결과를 리뷰하게 할 수 있습니다.
- **승인 전에 확인할 것을 먼저 보여 줍니다.** 검토 바에 보호 경로 · 진행 중인 git 작업 · 예상
  충돌 · 검증 결과가 나오고, 필요하면 **hunk 단위로 골라** 적용합니다. 적용이 실패하면
  체크포인트로 되돌립니다.
- **터미널에서 하던 작업을 이어받습니다.** 이미 돌고 있던 Claude Code 세션을 골라 새 작업으로
  가져오므로 문맥을 다시 설명하지 않아도 됩니다. 같은 세션을 두 곳에서 동시에 열려고 하면 거절합니다.
- **모든 것이 로컬에서 동작합니다.** 앱 · 저장소 · 메모리 파일 · 색인 DB가 전부 사용자의 기계에
  있고, 별도의 서버 계정이 필요 없습니다.

## 주요 기능

전체 목록은 [기능 카탈로그](docs/guide/features.md)에 있습니다.

| 영역 | 기능 |
|---|---|
| 작업 오케스트레이션 | worktree 격리 · 동시 실행 · 벤더 선택 · 직접 실행 모드 · 터미널 세션 이어받기 |
| 대화 | 프롬프트 대기열 · 메인 대화와 동시에 도는 별도 질문 · 체크포인트 되감기 · 에이전트의 되묻기 |
| 검토와 승인 | diff 뷰어 · hunk 부분 적용 · 라인 주석 · 벤더 리뷰 · 승인 준비도 · 충돌 해소 |
| 검증 | build/test 게이트 · 목표 계약(보호 경로 강제) · 결정 원장(선택) |
| 코드 | Monaco 에디터 · 정의/사용처 이동 · Quick Open · parquet 표 · IPython 콘솔 · 에이전트가 직접 조작하는 프리뷰 창 |
| 지식 | 파일 기반 메모리 자동 주입 · Wiki(2D/3D 그래프 · 연결 근거 · 순위 검색) · 코드 Wiki 생성 · 대기 중 복습 퀴즈 |
| 에이전트 환경 | 벤더 중립 `/스킬` · MCP · LSP 브리지 · 지시문 인터뷰 |
| 모바일 · 기타 | 모바일 PWA와 Web Push · 음성 입력 · 테마 25종 |

## 설치

### 준비물

| 필요한 것 | 확인 방법 |
|---|---|
| git과 로컬에 clone한 저장소 | `git --version` |
| 에이전트 CLI 하나 이상 | `claude --version` · `codex --version` · `agy --version` |

에이전트 CLI는 각자 로그인이나 API 키 설정을 마친 상태여야 합니다. **터미널에서 동작하지 않으면
Praxis에서도 동작하지 않습니다.** Praxis가 CLI를 대신 설치하지는 않습니다.

### macOS (Apple Silicon)

1. [릴리스 페이지](https://github.com/JaehaSS/praxis/releases/latest)에서 `Praxis_<버전>_aarch64.dmg`를 받습니다.
2. dmg를 열고 `Praxis.app`을 `응용 프로그램` 폴더로 옮깁니다.
3. 처음 열 때는 더블클릭하지 말고 **우클릭 → 열기**를 누른 뒤, 경고 창에서 다시 **열기**를 누릅니다.

3번이 필요한 이유는 이 빌드에 Apple 코드 서명과 공증이 없기 때문입니다. 명령행에서는
`xattr -dr com.apple.quarantine /Applications/Praxis.app`이 같은 일을 합니다. 받은 파일은
릴리스에 함께 올린 `SHA256SUMS.txt`와 `shasum -a 256`으로 대조할 수 있습니다.

### 소스에서 빌드

다른 환경이거나 서명 경고를 피하고 싶다면 직접 빌드합니다. 위의 준비물에 더해 다음이 필요합니다.

| 필요한 것 | 비고 |
|---|---|
| Node.js 22 또는 24 | 25에서는 테스트가 실패할 수 있습니다 |
| Rust stable + Clippy | |
| [Tauri 사전 요구 사항](https://v2.tauri.app/start/prerequisites/) | OS별 시스템 라이브러리 |

```sh
npm ci
npm run tauri build
```

macOS에서는 `src-tauri/target/release/bundle/` 아래에 `macos/Praxis.app`과 `dmg/`가 생깁니다.

## 첫 작업

처음 쓴다면 [사용 가이드](docs/guide/README.md)를 따라가면 됩니다.

- [시작하기](docs/guide/getting-started.md) — 준비물 확인부터 첫 작업 승인까지
- [기능 카탈로그](docs/guide/features.md) — 어떤 기능이 있는지
- [워크플로우 & FAQ](docs/guide/workflows-faq.md) — 자주 쓰는 작업 패턴과 문제 해결

## 지원 범위

| 항목 | 상태 |
|---|---|
| macOS (Apple Silicon) | 개발 · 빌드 · 사용이 이뤄지는 환경이고, 릴리스로 dmg를 배포합니다 |
| macOS (Intel) | 배포하는 빌드가 없습니다. 소스에서 빌드해야 합니다 |
| Windows · Linux | 코드 분기와 절차 문서는 있으나 **빌드 · 검증 기록이 없습니다** |
| 실행 위치 | 로컬 한 곳입니다. 원격 Linux Runner로 실행하던 경로는 2026-09-19에 제거했습니다 |
| 자동 업데이트 | 없습니다. 새 버전은 릴리스 페이지에서 받습니다 |

알려진 제약은 [기능 카탈로그의 "범위 밖"](docs/guide/features.md#범위-밖--주의할-것)에 모아 두었습니다.

## 개발

개발 중에는 `npm run tauri dev`가 유일한 진입점입니다. 프론트엔드를 따로 띄우는 스크립트는 없습니다.

CI는 없고 검사는 로컬에서 실행합니다.

```sh
npm run check       # 프론트엔드(tsc · vitest) + Rust 테스트 + clippy
```

## 더 읽을 것

[사용 가이드](docs/guide/README.md) · [docs/architecture.md](docs/architecture.md) ·
[DESIGN.md](DESIGN.md)(디자인 시스템) · [THIRD-PARTY-ASSETS.md](THIRD-PARTY-ASSETS.md)

설계 기록 · 결정 기록 · 작업 원장은 이 저장소에 싣지 않았습니다. 여기 실린 문서가 그 기록을
가리키던 곳은 링크 없이 제목만 남겨 두었습니다.

## 라이선스

[MIT](LICENSE). 파일 타입 아이콘의 출처와 저작권 표시는 [THIRD-PARTY-ASSETS.md](THIRD-PARTY-ASSETS.md)에 있습니다.
