// Web Push 구독 — 브라우저 쪽 절반. (설계 0013 §8)
//
// 서버는 페이로드 없는 푸시만 보낸다. Service Worker가 깨어나 `/v1`을 다시 조회해
// 알림 문구를 만든다. 여기서는 구독 등록·해제와 **상태 판독**만 다룬다.
//
// iOS는 홈 화면에 추가하기 전에는 구독 자체가 불가능하고, 권한이 조용히 철회될 수 있다.
// 그래서 "왜 안 되는지"를 상태로 분해해 화면에 드러내는 것이 이 모듈의 핵심이다.

export type PushState =
  /** 브라우저가 Push API를 제공하지 않는다(대개 secure context가 아니거나 iOS 홈화면 미추가). */
  | { kind: "unsupported"; reason: string }
  /** 쓸 수는 있지만 아직 구독하지 않았다. */
  | { kind: "idle" }
  /** 사용자가 권한을 거부했다 — 앱에서 되돌릴 수 없고 브라우저 설정에서 풀어야 한다. */
  | { kind: "denied" }
  | { kind: "subscribed" }
  | { kind: "error"; message: string };

/** iOS에서 홈 화면에 추가된 상태로 실행 중인지. 아니면 푸시 구독이 불가능하다. */
export function isStandalone(): boolean {
  if (typeof window === "undefined") return false;
  const legacy = (window.navigator as { standalone?: boolean }).standalone === true;
  return legacy || window.matchMedia?.("(display-mode: standalone)").matches === true;
}

/** iOS Safari 계열인지 — 홈 화면 추가 안내를 띄울지 판단한다. */
export function isIos(): boolean {
  if (typeof navigator === "undefined") return false;
  return /iPad|iPhone|iPod/.test(navigator.userAgent);
}

/** 지금 왜 구독할 수 없는지를 한 문장으로. 지원되면 null. */
export function unsupportedReason(): string | null {
  if (typeof window === "undefined") return "브라우저 환경이 아닙니다.";
  if (!window.isSecureContext) return "보안 연결(HTTPS)이 아니어서 알림을 쓸 수 없습니다.";
  if (!("serviceWorker" in navigator)) return "이 브라우저는 Service Worker를 지원하지 않습니다.";
  if (!("PushManager" in window)) {
    return isIos() && !isStandalone()
      ? "iOS는 홈 화면에 추가한 뒤에만 알림을 받을 수 있습니다."
      : "이 브라우저는 웹 푸시를 지원하지 않습니다.";
  }
  return null;
}

export async function currentState(): Promise<PushState> {
  const reason = unsupportedReason();
  if (reason) return { kind: "unsupported", reason };
  if (Notification.permission === "denied") return { kind: "denied" };
  try {
    const registration = await navigator.serviceWorker.ready;
    const subscription = await registration.pushManager.getSubscription();
    return subscription ? { kind: "subscribed" } : { kind: "idle" };
  } catch (cause: unknown) {
    return { kind: "error", message: cause instanceof Error ? cause.message : String(cause) };
  }
}

/** base64url VAPID 공개키 → applicationServerKey가 요구하는 Uint8Array. */
export function decodeBase64Url(value: string): Uint8Array {
  const padded = value.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(padded.padEnd(padded.length + ((4 - (padded.length % 4)) % 4), "="));
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

export async function subscribe(): Promise<PushState> {
  const reason = unsupportedReason();
  if (reason) return { kind: "unsupported", reason };
  try {
    const permission = await Notification.requestPermission();
    if (permission !== "granted") return { kind: "denied" };

    const keyResponse = await fetch("/v1/push/key", { credentials: "same-origin" });
    if (!keyResponse.ok) {
      return { kind: "error", message: `서버 키를 받지 못했습니다 (${keyResponse.status})` };
    }
    const { public_key: publicKey } = (await keyResponse.json()) as { public_key: string };

    const registration = await navigator.serviceWorker.ready;
    const subscription = await registration.pushManager.subscribe({
      // 페이로드를 쓰지 않아도 이 값은 필수다 — 사용자에게 보이는 알림만 허용한다는 선언.
      userVisibleOnly: true,
      applicationServerKey: decodeBase64Url(publicKey),
    });

    const saved = await fetch("/v1/push/subscribe", {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ endpoint: subscription.endpoint }),
    });
    if (!saved.ok) {
      // 서버에 남지 않은 구독은 아무 알림도 받지 못한다 — 브라우저 쪽도 되돌린다.
      await subscription.unsubscribe().catch(() => undefined);
      return { kind: "error", message: `구독을 저장하지 못했습니다 (${saved.status})` };
    }
    return { kind: "subscribed" };
  } catch (cause: unknown) {
    return { kind: "error", message: cause instanceof Error ? cause.message : String(cause) };
  }
}

export async function unsubscribe(): Promise<PushState> {
  try {
    const registration = await navigator.serviceWorker.ready;
    const subscription = await registration.pushManager.getSubscription();
    if (subscription) {
      await fetch("/v1/push/subscribe", {
        method: "DELETE",
        credentials: "same-origin",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ endpoint: subscription.endpoint }),
      }).catch(() => undefined);
      await subscription.unsubscribe();
    }
    return { kind: "idle" };
  } catch (cause: unknown) {
    return { kind: "error", message: cause instanceof Error ? cause.message : String(cause) };
  }
}

/** 상태 → 화면 문구. 실패를 조용히 두지 않는 것이 요지다. */
export function describePush(state: PushState): { label: string; detail?: string } {
  switch (state.kind) {
    case "unsupported":
      return { label: "알림 사용 불가", detail: state.reason };
    case "idle":
      return { label: "알림 꺼짐", detail: "검토 대기가 생겨도 폰이 울리지 않습니다." };
    case "denied":
      return {
        label: "알림 차단됨",
        detail: "브라우저 사이트 설정에서 알림을 허용해야 합니다.",
      };
    case "subscribed":
      return { label: "알림 켜짐" };
    case "error":
      return { label: "알림 설정 실패", detail: state.message };
  }
}
