# 프로젝트 학습 기록과 이벤트 파일

프로젝트의 학습은 공용 `project-learning.py`로 기록하고 조회한다. 새 학습·폐기는 `docs/learnings/`의 이벤트 파일에 보존한다. 파일을 직접 편집하거나 JSONL 끝에 추가하지 않는다.

## 기록과 조회

```bash
python3 "$HOME/.agent-harness/scripts/project-learning.py" recall --project . --query "현재 문제" --limit 3

python3 "$HOME/.agent-harness/scripts/project-learning.py" record --project . --input lesson.json

python3 "$HOME/.agent-harness/scripts/project-learning.py" retire --project . --id learning-ID --reason "더 이상 적용되지 않는 이유"
```

`record` 입력은 기존과 같다. `trigger`, `condition`, `failed_approach`, `correction`, `evidence`가 필수이고 `tags`는 선택이다. 비밀이나 고객 식별자·원문 로그를 넣지 않는다. 같은 활성 학습 내용의 재기록은 기존 ID를 반환한다.

`retire`는 원래 학습을 지우지 않고 별도 폐기 이벤트를 남긴다. 이후 조회는 폐기 대상을 제외한다. 학습 ID와 폐기 이벤트의 파일 식별자는 서로 다르다. 파일 이름은 SHA-256 해시이며, 파일 안의 `event`가 원본 학습·폐기 기록이다. `version: 1`은 저장 형식 버전이고 `legacy_index`는 이전 이벤트의 원본 행 순서다.

## 기존 원장 이전

```bash
python3 "$HOME/.agent-harness/scripts/project-learning.py" migrate --project .
```

이전은 `docs/learnings.jsonl`의 유효한 원본 이벤트를 새 파일로 복사한다. 원본은 그대로 남으며 명령을 다시 실행해도 같은 사건을 중복 생성하지 않는다. 원본에 잘못된 행이 있거나 정규화한 JSON이 크기 제한을 넘으면 발행 전에 실패한다. 기존 JSONL 행 제한은 32,768바이트이며 이벤트 파일은 메타데이터 공간 256바이트를 추가로 허용한다. 오류가 나면 원본을 보존한 채 원인을 확인한다.

공용 도구는 기존 JSONL과 이벤트 파일을 함께 읽는다. 동일 사건은 한 번만 처리하고, 같은 학습 ID의 다른 내용은 오류로 드러낸다. 기존 JSONL은 **고정된 호환 입력**이다. 새 이벤트를 합쳐 그 파일에 다시 쓰면 공통 파일의 병합 충돌이 돌아온다.

Git에는 이벤트 JSON과 정적 `.gitignore`를 포함한다. 잠금·임시 파일은 포함하지 않는다. 서로 다른 사건은 서로 다른 파일을 추가하므로 새 클론·worktree에서 별도 merge driver 없이 공유할 수 있다.

## 오래된 브랜치와 호스트

Codex와 다른 설치 호스트가 같은 공용 helper를 사용하면 동일 저장 계약을 따른다. 별도 컴퓨터나 과거 복사본의 helper는 자동으로 업그레이드되지 않는다. **모든 writer가 새 helper를 써야 신규 기록의 공통 JSONL 충돌이 사라진다.**

오래된 writer가 JSONL에 추가한 기록도 새 helper가 함께 읽는다. 다만 그 브랜치들 사이 JSONL 병합 충돌까지 없어지는 것은 아니다. 이전은 한 통합 브랜치에서 완료한 뒤 새 작업들이 그 전환을 받아 시작하는 방식이 적절하다.

## 복구와 무결성

- 이벤트 파일은 불변 원본이다. 같은 ID의 내용 불일치를 `ours` 또는 `theirs`로 일괄 덮어쓰지 않는다.
- 조회 응답의 `skipped`가 0보다 크면 legacy의 건너뛴 행을 확인한다. 조회 성공만으로 파일 전체가 정상이라는 뜻은 아니다.
- 구버전 helper로 돌아가기 전에는 신규 쓰기를 멈추고 원본 JSONL과 새 이벤트를 중복·폐기 의미에 따라 검증해 하나의 JSONL로 내보내야 한다. 원래 JSONL만 남기고 새 파일을 지우면 이후 기록이 사라진다.

```bash
python3 "$HOME/.agent-harness/scripts/project-learning.py" export --project . --output learning-recovery.jsonl
```

`export`는 전체 이력을 구버전 JSONL 형식으로 내보낸다. 대상 경로는 아직 없는 파일이어야 하며, 기존 파일·symlink는 덮어쓰지 않는다. 원본에 오류가 있으면 출력하지 않는다.

복구 시에는 모든 writer를 멈추고 기존 JSONL·이벤트·helper를 백업한다. export 파일만 넣은 임시 프로젝트를 구형 helper로 조회해 기록 집합·폐기 상태·정렬이 같은지 확인한다. 검증을 통과한 뒤에만 출력 파일을 기존 JSONL 경로로 원자적으로 교체하고 구형 helper를 복원한다. 검증 불일치 시 교체하지 않는다. 자동 rollback은 제공하지 않으며, 이벤트 파일과 백업은 복구 확인이 끝날 때까지 보존한다.

현재 이 컴퓨터의 공용 helper에는 적용이 끝났다. 프로젝트 Git만 복제해도 다른 컴퓨터의 공용 helper가 자동 설치되는 것은 아니다. 공용 코드·테스트·프로토콜 변경은 [전달 패치](../../scripts/harness-patches/project-learning-events.patch)로 보존했으며, 수정 전 소스 복사본에 적용해 현재 코드와 같은지 검증했다. 다른 설치 환경에서는 기존 버전과 패치 내용을 확인한 뒤 적용해야 한다.

설계·검증 기준은 구현 계획, 실제 결과는 구현·검증 기록에 있다.
