# 서드파티 에셋

## 파일 타입 아이콘 — material-icon-theme (MIT)

`src/components/ide/file-glyphs.ts`의 39개 SVG 경로는
[material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme)
(Philipp Kief, **MIT**)에서 가져왔다. VS Code에서 파일 트리 아이콘으로 쓰이는 바로 그 세트다.

- **패키지를 의존성에 두지 않는다.** 1251개 중 39개만 쓰고 각각 16×16 경로 한둘이라
  통째로 인라인해도 21KB다. 런타임 의존성도 폰트 로딩도 생기지 않는다(local-first)
- 추출은 `scripts/oneshot/extract-file-icons.mjs`. 아이콘을 더하려면 그 스크립트의
  `WANTED`에 이름을 넣고 다시 돌린다 — `file-glyphs.ts`는 생성물이라 손으로 고치지 않는다
- MIT는 저작권 표시 보존을 요구한다. **이 절이 그 표시다** — 지우지 말 것

> Google의 Material Symbols(Apache-2.0)와 다른 물건이다. 그쪽은 UI 아이콘 세트라
> 언어별 파일 아이콘이 없다. "vscode처럼"이 가리키는 것은 이쪽이다.

---

## 그 외 — 서드파티 에셋 없음

`src/assets/village/`·`src/assets/agents/`의 픽셀
자산은 전부 `tools/sprites/`의 굽기 스크립트가 코드로 생성한 것이라 라이선스 의무가 없다.

| 산출물 | 굽는 스크립트 |
|---|---|
| `src/assets/village/*.png` (지면·건물 3겹) | `tools/sprites/build-village-scene.py` |
| `src/assets/agents/praxis-agent-sprites.png` (역할 6종 × 프레임 7) | `tools/sprites/build-village-characters.py` |

팔레트는 `src/index.css`의 `.dark` 토큰에서 가져온다. 자산이 UI에서 떠 보이면 그쪽을
먼저 볼 것.

## 왜 사는 대신 그렸나

계획 0042는 LimeZu 팩 3종($5~7)을 크롭할 예정이었다. 구매하지 않기로 하면서 CC0 대안인
[Kenney Tiny Town](https://kenney.nl/assets/tiny-town)을 받아 확인했는데 **중세 RPG 마을**
이라 책상·모니터·회의 테이블이 하나도 없었다 — 소재가 없어 "작업 중 캐릭터는 책상 앞에
앉은 프레임"을 만들 수 없다. 팩을 섞는 쪽도 명암·채도·비율이 어긋나는 무료 에셋 조합의
고질적 실패 모드에 걸린다.

직접 찍으면 팔레트가 UI와 한 톤이고, 라이선스 조건도 사라진다. 저해상도(28×56 도트)라
손 도트 작업과 달리 파라미터화가 되는 것이 결정적이었다.

## 나중에 서드파티 팩을 도입한다면

`tools/sprites/vendor/`는 `.gitignore`에 있다. 굽기 **입력**은 저장소에 넣지 않고 로컬에만
두라는 뜻이다 — 대부분의 상용 팩이 재배포를 금지하기 때문이다. 커밋되는 것은 이 앱 전용
산출물뿐이어야 한다. 크레딧 의무가 있는 팩을 쓰면 앱 정보 화면에 표기 위치를 만들고
이 파일에 팩·저작자·라이선스·출처를 표로 남길 것.
