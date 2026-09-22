/** Python 콘솔에 흘려보낼 코드 조각. */

/** 워크트리 상대 경로를 절대 경로로. 이미 절대면(작업 폴더 밖 읽기 전용 탭) 그대로 둔다. */
export function absoluteWorktreePath(root: string, path: string): string {
  if (path.startsWith("/")) return path;
  const base = root.endsWith("/") ? root.slice(0, -1) : root;
  return `${base}/${path}`;
}

/** Python 문자열 리터럴 — JSON 이스케이프는 Python이 그대로 읽는다(`\"`·`\\`·`\uXXXX`). */
export function pythonStringLiteral(value: string): string {
  return JSON.stringify(value);
}

/** 표 파일을 pandas DataFrame `df`로 여는 셀. 마지막 줄의 `df`가 IPython의 Out[]로 표를 그린다. */
export function pandasOpenSnippet(root: string, path: string): string {
  const literal = pythonStringLiteral(absoluteWorktreePath(root, path));
  return `import pandas as pd\ndf = pd.read_parquet(${literal})\ndf`;
}
