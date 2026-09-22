/** 꼬리를 확장자 사슬로 인정하는 상한. 넘으면 버전 번호(`Praxis_0.1.0_aarch64.dmg`)를
 *  확장자로 오인한 것이라 보고 마지막 `.`까지 물러선다. */
const MAX_EXTENSION_TAIL = 10;

/** 표시 라벨을 머리·꼬리로 가른다. 꼬리는 줄지 않게 렌더해 구분자가 살아남는다.
 *
 *  끝에서 자르면 `docker-compose.yml`과 `docker-compose.prod.yml`이 둘 다 `docker-co…`가 되어
 *  구분이 사라진다 — 두 행을 가르는 정보가 하필 접미사에 있기 때문이다. */
export function splitLabel(label: string): [head: string, tail: string] {
  const slash = label.lastIndexOf("/");
  // 압축된 사슬 라벨(`src/main/java`)은 가장 깊은 폴더가 그 행의 정체다. 길이 상한을 두지 않는다.
  if (slash >= 0) return keepHead(label, slash);

  const last = label.lastIndexOf(".");
  // 선두 `.`은 확장자 구분자가 아니다 — `.gitignore`는 통째로 이름이다.
  if (last <= 0) return [label, ""];

  const prev = label.lastIndexOf(".", last - 1);
  if (prev > 0 && label.length - prev <= MAX_EXTENSION_TAIL) return keepHead(label, prev);
  return keepHead(label, last);
}

/** 꼬리만 남는 행은 만들지 않는다 — 머리가 비면 가르지 않은 것과 같다. */
function keepHead(label: string, at: number): [string, string] {
  if (at === 0) return [label, ""];
  return [label.slice(0, at), label.slice(at)];
}
