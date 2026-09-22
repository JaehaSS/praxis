import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { EditorWindow } from "./components/ide/EditorWindow";
import { ProjectEditorWindow } from "./components/ide/ProjectEditorWindow";
import { PreviewToolbarWindow } from "./components/ide/PreviewToolbarWindow";
import { isEditorEntry, isProjectEditorEntry } from "./lib/editor-window-events";
import { bootCachedCustomTheme, bootCustomThemes } from "./lib/theme-files";
import { startThemeBroadcast, startThemeFollower } from "./lib/theme-sync";
import { applyTheme, loadThemeId } from "./lib/themes";
import { isPreviewToolbarEntry } from "./lib/preview-workbench/window-events";
import "./index.css";

const editorEntry = isEditorEntry(window.location.search);
const projectEditorEntry = isProjectEditorEntry(window.location.search);
const toolbarEntry = isPreviewToolbarEntry(window.location.search);

// 저장된 테마 적용 (기본 Praxis Dark). React 마운트 전에 불러야 첫 프레임이 깜빡이지 않는다.
// 이진 토글 시절의 "light"/"dark" 값은 loadThemeId가 흡수하고, 여기서 새 id로 다시 저장된다.
const themeId = loadThemeId();
// 커스텀 테마는 파일에서 오므로 비동기다 — 캐시 spec을 동기 등록해 첫 프레임을 맞추고,
// 정본 로드는 그 뒤에 따라온다. 캐시가 없으면 getTheme 폴백으로 기본 테마가 뜬다.
if (toolbarEntry) bootCachedCustomTheme(themeId);
else bootCustomThemes(themeId);
applyTheme(themeId);
// 두 창은 같은 localStorage를 읽으므로 부팅 직후에는 색이 맞다. 어긋나는 것은 그 뒤다 —
// 에디터 창은 앱 수명 내내 재부팅되지 않으므로 메인 창의 이후 변경을 이벤트로 받아야 한다.
if (editorEntry || projectEditorEntry) startThemeFollower();
else if (!toolbarEntry) startThemeBroadcast();
// StrictMode 비활성: dev 이중 마운트로 PTY가 중복 spawn되는 것을 방지 (Phase 0).
const rootView = toolbarEntry ? <PreviewToolbarWindow /> : editorEntry ? <EditorWindow /> : projectEditorEntry ? <ProjectEditorWindow /> : <App />;
// 네 진입 뷰를 한 곳에서 감싼다 — 렌더 예외로 트리가 통째로 사라져 빈 화면이 되는 대신
// 무엇이 터졌는지 화면에 남긴다.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <ErrorBoundary>{rootView}</ErrorBoundary>,
);
