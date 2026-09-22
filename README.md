# Praxis

**AI 코딩 에이전트를 여러 개 동시에 굴리고, 바뀐 것을 직접 보고 승인하는 데스크톱 IDE.**

에이전트에게 일을 시키는 것 자체는 터미널에서도 됩니다. Praxis가 맡는 건 그다음입니다 —
작업마다 **격리된 git worktree**를 파고, 에이전트를 그 안에서만 움직이게 하고, 끝나면 diff를
보여준 뒤 **승인할지 버릴지 사람이 정하게** 합니다. 승인 전까지 원본 브랜치는 한 글자도
바뀌지 않습니다.

<!-- 스크린샷 자리: 홈 화면 1장 + 검토/diff 화면 1장. 아직 없음 — 캡처해서 docs/assets/에 넣고 여기 링크할 것. -->

```
지시문  →  격리 worktree 생성  →  에이전트 실행  →  diff 검토  →  승인 또는 폐기
           praxis/<작업명> 브랜치                     (부분 적용 가능)
```

## 무엇이 다른가

- **원본을 건드리지 않는다** — 작업마다 `.praxis/worktrees/` 아래 worktree와 전용 브랜치.
  폐기하면 worktree째 사라지고, 승인해야 커밋·머지가 일어납니다.
- **한 번에 여러 개** — 기본 8개(1~64 설정)를 각자의 worktree와 터미널 세션에서 병렬로 돌립니다.
- **벤더를 섞어 쓴다** — Claude Code · Codex · Antigravity(Gemini)를 작업마다 고르고, 같은 지시문을
  여럿에게 시켜 결과를 비교(앙상블)하거나 서로 리뷰시킬 수 있습니다.
- **승인이 체크리스트다** — 검토 바에서 보호 경로·진행 중인 git 작업·예상 충돌·검증 결과를 먼저
  보고, 필요하면 **hunk 단위로만** 골라 적용합니다. 적용이 실패하면 체크포인트로 되돌립니다.
- **터미널에서 하던 것을 그대로 가져온다** — 이미 돌던 Claude Code 세션을 골라 새 작업으로
  이어받습니다. 문맥을 다시 설명할 필요가 없고, 같은 세션을 두 곳에서 물면 거절합니다.
- **전부 로컬에서 돈다** — 앱, 저장소, 메모리 파일, 색인 DB가 전부 내 기계에 있습니다. 별도
  서버 계정이 없습니다.

## 기능

전체 목록은 **[기능 카탈로그](docs/guide/features.md)** 에 있습니다. 굵직한 것만 옮기면:

| | |
|---|---|
| 작업 오케스트레이션 | worktree 격리 · 동시 실행 · 벤더 선택 · 직접 실행 모드 · 터미널 세션 이어받기 |
| 대화 | 프롬프트 대기열 · 메인과 동시에 도는 따로 질문 · 체크포인트 되감기 · 에이전트가 되묻기 |
| 검토와 승인 | diff 뷰어 · hunk 부분 적용 · 라인 주석 · 벤더 리뷰 · 승인 준비도 · 충돌 해소 |
| 검증 | build/test 게이트 · 목표 계약(보호 경로 강제) · 결정 원장(선택) |
| 코드 | Monaco 에디터 · 정의/사용처 이동 · Quick Open · parquet 표·IPython 콘솔 · 프리뷰 창을 에이전트가 직접 조작 |
| 지식 | 파일 기반 메모리 자동 주입 · Wiki(2D/3D 그래프·연결 근거 리더·순위 검색) · 코드 Wiki 생성 · 대기 중 복습 퀴즈와 내 글 카드 |
| 에이전트 환경 | 벤더 중립 `/스킬` · MCP·LSP 브리지 · 지시문 인터뷰(깊게 파기 → 채점) |
| 바깥 | 모바일 PWA와 Web Push · 음성 입력 · 테마 25종 |

## 시작하기

처음 쓰신다면 **[사용 가이드](docs/guide/README.md)** 부터 보세요.

- [시작하기](docs/guide/getting-started.md) — 준비물부터 첫 작업 승인까지
- [기능 카탈로그](docs/guide/features.md) — 어떤 기능이 있는지
- [워크플로우 & FAQ](docs/guide/workflows-faq.md) — 작업 패턴과 문제 해결

### 미리 있어야 하는 것

| | 확인 |
|---|---|
| git과 로컬에 clone된 저장소 | `git --version` |
| 에이전트 CLI 하나 이상 | `claude --version` / `codex --version` / `agy --version` |
| Node.js 22 또는 24 | 25는 테스트가 실패할 수 있습니다 |
| Rust stable + Clippy + Tauri 사전 요구 사항 | 빌드할 때 |

에이전트 CLI는 각자 로그인·API 키 설정이 끝나 있어야 합니다. **터미널에서 안 되면 Praxis에서도
안 됩니다.** Praxis가 CLI를 대신 설치하지는 않습니다.

### 설치

macOS(Apple Silicon)는 [릴리스](https://github.com/JaehaSS/praxis/releases/latest)에서 dmg를 받습니다.
Apple 서명·공증이 없어 **처음 열 때는 우클릭 → 열기**를 눌러야 합니다.

그 밖의 환경은 소스에서 빌드해 씁니다.

```sh
npm ci
npm run tauri build
```

macOS에서는 `src-tauri/target/release/bundle/macos/Praxis.app`과 같은 경로의 `dmg/`가 나옵니다.
개발 중에는 `npm run tauri dev`가 유일한 진입점입니다.

## 검증

CI는 없습니다. 검사는 로컬에서 돌립니다.

```sh
npm run check       # 프런트(tsc·vitest) + Rust 테스트 + clippy
```

## 지금 어디까지 와 있나

| 항목 | 상태 |
|---|---|
| macOS 데스크톱 | 개발·빌드·사용이 이뤄지는 환경 |
| Windows / Linux 데스크톱 | 코드 분기와 절차 문서는 있으나 **이 저장소에 빌드·검증 기록이 없습니다** |
| 실행 위치 | **로컬 한 곳입니다.** 원격 Linux Runner로 실행하던 경로는 2026-09-19에 앱에서 제거됐습니다 |
| 배포 자산 | macOS(Apple Silicon) dmg를 [릴리스](https://github.com/JaehaSS/praxis/releases/latest)로 게시했습니다. 다른 환경의 빌드 자산은 없습니다 |
| 코드 서명 | Apple 서명·공증이 없습니다(adhoc 서명). 내려받아 처음 열 때 **우클릭 → 열기**가 필요합니다 |
| 라이선스 | [MIT](LICENSE) |

알려진 제약은 [기능 카탈로그의 "범위 밖"](docs/guide/features.md#범위-밖--주의할-것)에 모아 두었습니다.

## 더 읽을 것

[사용 가이드](docs/guide/README.md) · [docs/architecture.md](docs/architecture.md) ·
[DESIGN.md](DESIGN.md)(디자인 시스템) · [THIRD-PARTY-ASSETS.md](THIRD-PARTY-ASSETS.md)

설계 기록·결정 기록·작업 원장은 개발 저장소에 있습니다. 여기 실린 문서가 옛 문서를 가리키는
곳은 링크 없이 제목만 남겨 두었습니다.
