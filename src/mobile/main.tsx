import ReactDOM from "react-dom/client";
import MobileApp from "./MobileApp";
import { registerServiceWorker } from "./sw-register";
import "../index.css";

// 모바일은 다크 고정 (DESIGN.md 터미널 다크 원칙 + 야외 가독성). 테마 토글은 범위 밖.
document.documentElement.classList.add("dark");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<MobileApp />);

// 등록 실패는 앱 동작을 막지 않는다 — SW가 없으면 푸시만 불가하고 나머지는 그대로다.
void registerServiceWorker();
