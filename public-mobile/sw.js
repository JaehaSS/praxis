// Praxis 모바일 Service Worker — 스코프 /m/ (설계 0013 §5.2·§8)
//
// 캐싱은 하지 않는다. 오프라인 셸은 Runner가 죽었을 때 살아있는 척하는 화면을 만들 뿐이고,
// 이 앱의 1순위 요구는 그 반대(가용성을 정확히 드러내기)다.
//
// 푸시는 **페이로드가 없다.** 여기서 동일 출처 `/v1`을 다시 조회해 문구를 만든다.
// 작업 내용이 FCM/Apple 서버를 지나가지 않고, 서버는 암호화 스택이 필요 없어진다.

const SW_VERSION = "m2-3";
const TAG = "praxis-review";

self.addEventListener("install", () => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

// 설치 가능(installable) 판정에 fetch 핸들러가 필요하다. 네트워크로 그대로 통과시키고
// 실패는 브라우저 기본 오류 화면에 맡긴다 — 연결 실패의 원인 판독은 앱이 /v1/health로 한다.
self.addEventListener("fetch", (event) => {
  if (event.request.mode !== "navigate") return;
  event.respondWith(fetch(event.request));
});

/** 내 결정을 기다리는 작업만 추린다. 진행 중 작업까지 알리면 알림이 무의미해진다. */
function awaiting(tasks) {
  return tasks.filter(
    (task) => task.state === "AwaitingReview" || task.state === "PendingApproval",
  );
}

async function buildNotification() {
  try {
    const response = await fetch("/v1/tasks", { credentials: "same-origin" });
    if (!response.ok) {
      // 세션이 끊겼으면 그 사실 자체가 알려야 할 정보다.
      if (response.status === 401) {
        return { title: "Praxis 연결이 끊겼습니다", body: "다시 페어링해야 합니다.", url: "/m/" };
      }
      return null;
    }
    const pending = awaiting(await response.json());
    if (pending.length === 0) return null;
    const [first] = pending.sort((a, b) => b.updated_at - a.updated_at);
    return {
      title:
        pending.length === 1 ? "검토 대기 1건" : `검토 대기 ${pending.length}건`,
      body: (first.instruction || "").slice(0, 120),
      url: `/m/t/${first.id}`,
    };
  } catch {
    return null;
  }
}

self.addEventListener("push", (event) => {
  event.waitUntil(
    (async () => {
      const notification = await buildNotification();
      // userVisibleOnly 구독이라 알림을 띄우지 않으면 브라우저가 경고를 남기거나
      // 권한을 회수할 수 있다. 조회에 실패해도 최소한의 알림은 띄운다.
      const payload = notification ?? {
        title: "Praxis",
        body: "작업 상태가 바뀌었습니다.",
        url: "/m/",
      };
      await self.registration.showNotification(payload.title, {
        body: payload.body,
        tag: TAG,
        renotify: true,
        data: { url: payload.url },
      });
    })(),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = event.notification.data?.url ?? "/m/";
  event.waitUntil(
    (async () => {
      const clients = await self.clients.matchAll({
        type: "window",
        includeUncontrolled: true,
      });
      // 이미 열려 있는 창이 있으면 새 탭을 만들지 않고 그 창을 옮긴다.
      for (const client of clients) {
        if (new URL(client.url).pathname.startsWith("/m/")) {
          await client.focus();
          if ("navigate" in client) await client.navigate(url).catch(() => undefined);
          return;
        }
      }
      await self.clients.openWindow(url);
    })(),
  );
});

self.addEventListener("message", (event) => {
  if (event.data === "version") {
    event.source?.postMessage({ version: SW_VERSION });
  }
});
