"""Read-only bridge to the installed harness; no code is imported from the vault."""

import json
import sys
from pathlib import Path

scripts, root = Path(sys.argv[1]), Path(sys.argv[2])
sys.path.insert(0, str(scripts))
import knowledge_graph as graph

original_files = graph._files


def bounded_files(folder):
    count = size = 0
    for path in original_files(folder):
        count += 1
        if path.is_symlink():
            continue
        size += path.stat().st_size
        if count > 1000 or size > 16 * 1024 * 1024:
            raise ValueError(
                "창고가 탐색 한도(1,000문서 / 16MiB)를 넘습니다. 더 작은 폴더를 연결하세요."
            )
        yield path


graph._files = bounded_files
result = graph.build_graph(root)
# 앱은 body를 직접 렌더하므로 하네스가 만든 html은 읽는 쪽이 없다.
# 그대로 두면 IPC 페이로드의 절반가량이 아무도 안 쓰는 문자열이 된다.
for node in result.get("nodes", []):
    node.pop("html", None)
encoded = json.dumps(result, ensure_ascii=False)
if len(encoded.encode("utf-8")) > 24 * 1024 * 1024:
    raise ValueError("그래프가 출력 한도를 넘습니다.")
print(encoded)
