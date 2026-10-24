import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";

// Keep limits named and aligned with the engine instead of spreading magic values through handlers.
const REFRESH_INTERVAL_MS = 3_000;
const PROGRESS_INTERVAL_MS = 500;
const MAX_MESSAGE_BYTES = 1024 * 1024;
const HISTORY_LIMIT = 100;
const LAST_DEVICE_KEY = "fta:last-device";
const PINNED_KEY = "fta:pinned";
type JsonObject = Record<string, unknown>;
// Each native window loads one view; only main owns delivery acknowledgements and engine startup.
const requestedView = new URLSearchParams(window.location.search).get("view");
const view = requestedView === "inbox" || requestedView === "settings" ? requestedView : "main";
const isMain = view === "main";
interface Device {
  device_id: string;
  display_name: string;
  os: string;
  online: boolean;
  paired: boolean;
}
interface TransferItem {
  name: string;
  saved_name?: string;
  path?: string;
  size: string;
  state: string;
  bytes_transferred?: string;
}
interface TransferRecord {
  transfer_id: string;
  peer_id: string;
  direction: string;
  kind: string;
  state: string;
  created_at: number;
  body?: string;
  items: TransferItem[];
  error?: string;
}
interface ReceiptEvent {
  sequence: number;
  transfer_id: string;
  kind: string;
  created_at: number;
  record: TransferRecord;
}

// Native views share trusted static templates; received content remains textContent and unused view controls are removed.
const root = document.querySelector<HTMLDivElement>("#app");
if (!root) throw new Error("앱 화면을 찾을 수 없습니다");
root.innerHTML = `
  <main class="shell">
    <header class="app-header" data-drag-handle>
      <div class="brand"><span class="brand-mark" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M4 7h15m-4-4 4 4-4 4M20 17H5m4 4-4-4 4-4"/></svg></span><h1>FTA</h1></div>
      <div class="window-actions"><button data-window="inbox" class="quiet icon-label" aria-label="기록" title="기록"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 12a9 9 0 1 0 3-6M3 3v6h6M12 7v5l3 2"/></svg></button><button data-window="settings" class="quiet icon-label" aria-label="설정" title="설정"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 3 1-1h4l1 3 2 1 3-1 2 4-2 2v2l2 2-2 4-3-1-2 1-1 3h-4l-1-3-2-1-3 1-2-4 2-2v-2L2 9l2-4 3 1 2-1Z"/><circle cx="12" cy="12" r="3"/></svg></button><button id="fold-window" class="quiet icon-label" aria-label="접기" title="접기"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14"/></svg></button><button id="pin" class="quiet icon-label" aria-pressed="false" title="항상 위에 표시" aria-label="항상 위에 표시"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 3 6 0-1 6 4 4H6l4-4-1-6Zm3 10v8"/></svg></button><button id="close-app-window" class="quiet icon-label" aria-label="창 닫기" title="창 닫기"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18"/></svg></button></div>
    </header>
    <div id="notice" class="notice" role="status" aria-live="polite" hidden></div>
    <section id="send-panel" class="panel send-board">
      <aside class="target-rail"><div class="section-heading"><h2>기기</h2><div class="device-tools"><button id="refresh" class="quiet icon-label" aria-label="새로고침" title="새로고침"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 7v5h-5M4 17v-5h5M6 7a7 7 0 0 1 12-1l2 6M4 12l2 6a7 7 0 0 0 12-1"/></svg></button><button id="add-device" class="quiet small" aria-label="기기 등록" title="기기 등록">+</button></div></div><div id="devices" class="devices" role="group" aria-label="전송 대상"></div></aside>
      <div class="send-workspace">
        <section class="file-section" aria-labelledby="file-heading"><h2 id="file-heading" class="sr-only">파일</h2><button id="choose-files" class="quiet small" aria-label="파일 선택">파일 선택</button><div id="file-selection" class="file-selection" hidden><ul id="file-list"></ul><div class="selection-actions"><button id="clear-files" class="quiet">선택 해제</button><button id="send-files" class="primary">파일 보내기</button></div></div></section>
        <section class="composer" aria-labelledby="message-heading"><h2 id="message-heading" class="sr-only">텍스트 · 링크</h2><label class="sr-only" for="message">전송할 텍스트 또는 URL</label><div class="message-input"><textarea id="message" rows="2" placeholder="텍스트 또는 URL"></textarea><button id="send-message" class="primary send-arrow" aria-label="보내기" title="보내기" disabled><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14m-6-6 6 6-6 6"/></svg></button></div></section>
        <div class="transfer-progress"><div id="transfer-status" class="transfer-status" role="status" hidden></div><progress id="transfer-progress-bar" value="0" max="100" aria-label="파일 전송 진행률" hidden></progress><button id="cancel-transfer" class="quiet" hidden>전송 취소</button></div>
      </div>
    </section>
    <section id="inbox-panel" class="utility-panel" aria-labelledby="inbox-title"><div class="utility-header" data-drag-handle><h2 id="inbox-title">기록</h2><button data-close-window="inbox" class="quiet" aria-label="기록 닫기">×</button></div><button id="reload-inbox" class="quiet small">새로고침</button><div id="history"></div></section>
    <section id="settings-panel" class="utility-panel" aria-labelledby="settings-title"><div class="utility-header" data-drag-handle><h2 id="settings-title">설정</h2><button data-close-window="settings" class="quiet" aria-label="설정 닫기">×</button></div>
      <form id="settings-form" class="settings-section"><div class="section-heading"><h2>이 컴퓨터</h2><span class="section-caption">파일 수신 및 연결</span></div><div class="settings-grid"><div><label for="setting-name">기기 이름</label><input id="setting-name" autocomplete="off" required /></div><div><label for="setting-folder">파일 수신 폴더</label><input id="setting-folder" autocomplete="off" required /></div><div><label for="setting-listen">수신 주소</label><input id="setting-listen" autocomplete="off" placeholder="IP:포트" required /></div><div><label for="setting-networks">허용 네트워크</label><input id="setting-networks" autocomplete="off" placeholder="CIDR, 쉼표로 구분" /></div></div><label class="checkbox-label"><input id="setting-discovery" type="checkbox" /> 같은 네트워크에서 기기 자동 발견</label><p class="help">네트워크 변경은 저장 후 서비스를 재시작하면 적용됩니다.</p><div class="form-actions"><button id="restart-engine" class="secondary" type="button">서비스 재시작</button><button class="primary" type="submit">설정 저장</button></div></form>
      <section class="settings-section" aria-label="앱 동작"><div class="settings-row"><div><strong>로그인 시 실행</strong><p>로그인하면 자동 수신을 시작합니다.</p></div><button id="autostart" class="secondary">확인 중</button></div><div class="settings-row"><div><strong>수신 알림</strong><p id="notification-status">알림 권한을 확인합니다.</p></div><button id="notification-permission" class="secondary">알림 허용</button></div></section>
      <section class="settings-section"><div class="section-heading"><h2>기기 등록</h2><button id="pair-open" class="secondary small">등록 모드 열기</button></div><p class="help">양쪽 기기의 비교값이 같은지 확인한 뒤 등록하세요.</p><label for="pair-target">상대 기기 또는 LAN 주소</label><div class="inline-field"><input id="pair-target" placeholder="기기 ID 또는 주소" /><button id="pair-connect" class="secondary">연결</button></div><section id="pair-details" class="pair-details" aria-live="polite" hidden><div class="section-heading"><strong id="pair-state">연결 대기</strong><span id="pair-peer"></span></div><p id="pair-address"></p><p id="pair-comparison" class="comparison-value"></p><p id="pair-expiry" class="help"></p></section><input id="pair-session" type="hidden" /><div class="form-actions"><button id="pair-cancel" class="quiet">취소</button><button id="pair-confirm" class="primary">비교값 일치 확인</button></div><div id="paired-devices"></div></section>
      <div class="quit-row"><p class="help">창을 닫아도 수신은 계속됩니다.</p><button id="quit" class="danger">앱 종료</button></div>
    </section>
  </main>
  <button id="expand-window" class="expand-widget" aria-label="펼치기" title="펼치기" hidden><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 9V5h4m6 0h4v4m0 6v4h-4M9 19H5v-4"/></svg></button>
  <dialog id="detail-dialog"><div class="detail-header"><h2 id="detail-title">수신 내용</h2><button id="close-detail" class="quiet">닫기</button></div><p id="detail-meta" class="help"></p><pre id="detail-body" class="detail-body"></pre><div id="detail-items"></div><div class="detail-actions"><button id="copy-detail" class="secondary">복사</button><button id="open-detail" class="primary" hidden>링크 열기</button><button id="reveal-detail" class="secondary" hidden>저장 위치 열기</button></div></dialog>
`;
// Detach unused view content rather than leaving another window's controls hidden and active.
const utilityPanel = root.querySelector<HTMLElement>(`#${view}-panel`);
if (isMain) {
  root.querySelector("#inbox-panel")?.remove();
  root.querySelector("#settings-panel")?.remove();
} else if (utilityPanel) {
  const notice = root.querySelector<HTMLElement>("#notice");
  const detail = view === "inbox" ? root.querySelector<HTMLDialogElement>("#detail-dialog") : null;
  root.replaceChildren(utilityPanel);
  utilityPanel.classList.add("utility-window");
  if (notice) utilityPanel.querySelector(".utility-header")?.after(notice);
  if (detail) root.append(detail);
}
// Shared handlers bind only to controls belonging to this native view.
function on(id: string, type: string, handler: EventListener): void {
  document.getElementById(id)?.addEventListener(type, handler);
}

// Fail fast on static template mistakes instead of letting handlers silently address missing elements.
function element<T extends HTMLElement = HTMLElement>(id: string): T {
  const result = document.getElementById(id);
  if (!result) throw new Error(`화면 요소를 찾을 수 없습니다: ${id}`);
  return result as T;
}

const desktop = isTauri();
let devices: Device[] = [];
let records: TransferRecord[] = [];
let selectedId = localStorage.getItem(LAST_DEVICE_KEY) ?? "";
let selectedPaths: string[] = [];
let sending = false;
let connected = false;
let networkReady = false;
let refreshing = false;
let autostartEnabled = false;
let currentDetail: TransferRecord | undefined;
let activePairSession = "";
let inFlightId = "";
let progressTimer: number | undefined;
let activationProcessing = false;
const receiptInFlight = new Set<number>();

// Errors remain visible without echoing arbitrary received content into HTML.
function showNotice(message: string, error = false, source = "action"): void {
  const notice = element("notice");
  // Keep connection recovery from clearing an unrelated transfer or permission failure.
  notice.dataset.source = source;
  notice.textContent = message;
  notice.classList.toggle("error", error);
  notice.hidden = false;
  positionNotice();
}

// Keep feedback in the current native view, or above an open received-record detail sheet.
function positionNotice(): void {
  const dialog = [...document.querySelectorAll<HTMLDialogElement>("dialog[open]")].at(-1);
  const anchor = dialog?.querySelector(".detail-header") ?? document.querySelector(".utility-header, .app-header");
  anchor?.after(element("notice"));
}

// Translate stable engine codes into corrective actions while preserving unexpected native diagnostics.
function userError(error: unknown): string {
  const text = String(error);
  const code = text.split(":", 1)[0] ?? "";
  const messages: Record<string, string> = {
    SERVICE_UNAVAILABLE: "전송 서비스에 연결할 수 없습니다. 설정에서 서비스를 재시작하세요.",
    OFFLINE: "상대 기기에 연결할 수 없습니다. 기기가 켜져 있고 같은 네트워크에 있는지 확인하세요.",
    TIMEOUT: "연결 응답 시간이 초과되었습니다. 기기 연결 상태를 확인하세요.",
    NETWORK_CHANGED: "집 네트워크가 변경되어 전송을 중지했습니다. 설정 확인 후 서비스를 재시작하세요.",
    NETWORK_DENIED: "허용한 집 네트워크의 주소가 아닙니다. 연결 설정을 확인하세요.",
    NO_SPACE: "받는 기기의 저장 공간이 부족합니다.",
    PERMISSION_DENIED: "파일 또는 수신 폴더의 접근 권한을 확인하세요.",
    BUSY: "다른 작업을 처리하고 있습니다. 현재 작업이 끝난 뒤 다시 시도하세요.",
    UNTRUSTED: "등록되지 않은 기기입니다. 기기를 먼저 등록하세요.",
    KEY_CHANGED: "기기 인증 정보가 변경되었습니다. 기존 등록을 확인하고 다시 등록하세요.",
    NOT_FOUND: "항목을 찾을 수 없습니다. 기록이 삭제되었거나 더 이상 보관되지 않을 수 있습니다.",
    VERSION_UNSUPPORTED: "두 기기의 앱 버전이 호환되지 않습니다. 앱을 업데이트하세요.",
    CANCELLED: "전송을 취소했습니다. 이미 저장된 파일은 유지됩니다.",
    HASH_MISMATCH: "파일을 검증하지 못했습니다. 원본 파일을 확인한 뒤 다시 보내세요.",
  };
  return messages[code] ?? text;
}

// Every async UI handler reports rejection; an IPC error must never look like a successful send.
async function act(action: () => Promise<void>): Promise<void> {
  try { await action(); } catch (error) { showNotice(userError(error), true); }
}

// Browser preview is presentation-only and does not simulate a working engine.
async function rpc<T>(method: string, params: JsonObject = {}): Promise<T> {
  if (!desktop) throw new Error("전송 기능은 FTA 데스크톱 앱에서 사용할 수 있습니다.");
  return invoke<T>("engine_request", { method, params });
}

// Only online registered devices may become a target; a stale selection is cleared during refresh.
function selectedDevice(): Device | undefined {
  if (!networkReady) return undefined;
  return devices.find((device) => device.device_id === selectedId && device.online && device.paired);
}

// Keep the destination in accessible action names without duplicating the selected device row.
function updateActions(): void {
  if (!isMain) return;
  const target = selectedDevice();
  element("send-message").setAttribute("aria-label", target ? `${target.display_name}에 보내기` : "보내기");
  const bytes = new TextEncoder().encode(element<HTMLTextAreaElement>("message").value).length;
  element<HTMLButtonElement>("send-message").disabled = !target || !connected || sending || bytes === 0 || bytes > MAX_MESSAGE_BYTES;
  element<HTMLButtonElement>("send-files").disabled = !target || !connected || sending || selectedPaths.length === 0;
  element<HTMLButtonElement>("choose-files").disabled = !desktop || sending;
}

// Only application-owned SVG paths enter the DOM; received names remain text nodes.
function icon(name: "computer" | "file" | "text" | "link" | "delete"): SVGSVGElement {
  const paths = {
    computer: ["M3 4h18v12H3z", "M8 21h8m-4-5v5"],
    file: ["M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9l-6-6Zm0 0v6h6"],
    text: ["M4 5h16M4 10h16M4 15h10M4 20h7"],
    link: ["M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2"],
    delete: ["M4 7h16M9 7V4h6v3M6 7l1 14h10l1-14M10 11v6m4-6v6"],
  };
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  for (const value of paths[name]) {
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", value);
    svg.append(path);
  }
  return svg;
}

// Keep status vocabulary consistent across the list, detail sheet and active transfer.
function stateLabel(state: string): string {
  const labels: Record<string, string> = {
    completed: "완료", partial: "일부 완료", failed: "실패", cancelled: "취소됨", unknown: "확인 필요",
    sending: "전송 중", receiving: "수신 중", verifying: "검증 중", committing: "저장 중", pending: "준비 중",
  };
  return labels[state] ?? state;
}

// Horizontal targets show only a computer and its name; status stays available without extra visible labels.
function renderDevices(): void {
  if (!isMain) return;
  const list = element("devices");
  list.replaceChildren();
  if (!devices.length) {
    const empty = document.createElement("div");
    empty.className = "empty-device";
    const title = document.createElement("strong");
    title.textContent = "기기 없음";
    empty.append(title);
    list.append(empty);
  }
  for (const device of devices) {
    const button = document.createElement("button");
    button.className = "device-tile";
    button.dataset.deviceId = device.device_id;
    button.disabled = !device.online || !device.paired || sending || !networkReady;
    button.setAttribute("aria-pressed", String(device.device_id === selectedId));
    const status = device.paired ? (device.online ? "온라인" : "오프라인") : "등록 필요";
    button.title = `${device.display_name} · ${status}`;
    button.setAttribute("aria-label", `${device.display_name}, ${status}`);
    const deviceIcon = document.createElement("span");
    deviceIcon.className = "device-icon";
    deviceIcon.append(icon("computer"));
    const dot = document.createElement("span");
    dot.className = "device-status";
    dot.dataset.state = !device.paired ? "unpaired" : device.online && networkReady ? "online" : "offline";
    dot.setAttribute("aria-hidden", "true");
    deviceIcon.append(dot);
    const name = document.createElement("span");
    name.className = "device-name";
    name.textContent = device.display_name;
    button.append(deviceIcon, name);
    button.addEventListener("click", () => {
      selectedId = device.device_id;
      localStorage.setItem(LAST_DEVICE_KEY, selectedId);
      renderDevices();
      updateActions();
    });
    list.append(button);
  }
  updateActions();
  highlightDropTarget();
}

interface DropPoint { x: number; y: number }
let dragPoint: DropPoint | undefined;

// Native coordinates identify the tile under the cursor even when its icon/name is the hit element.
function dropTarget(point: DropPoint): Device | undefined {
  if (!connected || !networkReady || sending || document.querySelector("dialog[open]") !== null) return undefined;
  const tile = document.elementFromPoint(point.x, point.y)?.closest<HTMLButtonElement>(".device-tile");
  if (!tile || tile.disabled) return undefined;
  return devices.find((device) => device.device_id === tile.dataset.deviceId && device.online && device.paired);
}
function highlightDropTarget(): void {
  const target = dragPoint ? dropTarget(dragPoint) : undefined;
  for (const tile of document.querySelectorAll<HTMLButtonElement>(".device-tile")) {
    tile.classList.toggle("drop-target", tile.dataset.deviceId === target?.device_id);
  }
}

// File selections exist only in this UI session; they are not an offline delivery queue.
function renderFiles(): void {
  if (!isMain) return;
  element("file-selection").hidden = selectedPaths.length === 0;
  element("file-list").replaceChildren(...selectedPaths.map((path) => {
    const item = document.createElement("li");
    item.textContent = path.split(/[\\/]/).pop() ?? path;
    item.title = path;
    return item;
  }));
  updateActions();
}

// History rows distinguish file identity, peer, timestamp and outcome without rendering remote markup.
function renderHistory(): void {
  if (view !== "inbox") return;
  const list = element("history");
  list.replaceChildren();
  if (!records.length) {
    const empty = document.createElement("p");
    empty.className = "empty-history";
    empty.textContent = "전송 기록이 없습니다. 파일이나 메시지를 보내면 여기에 표시됩니다.";
    list.append(empty);
  }
  for (const record of records) {
    const row = document.createElement("article");
    row.className = "history-row";
    const open = document.createElement("button");
    open.className = "history-open";
    const recordIcon = document.createElement("span");
    recordIcon.className = "history-icon";
    recordIcon.append(icon(record.kind === "files" ? "file" : record.kind === "url" ? "link" : "text"));
    const copy = document.createElement("span");
    copy.className = "history-copy";
    const title = document.createElement("strong");
    title.textContent = record.kind === "files" ? (record.items.length === 1 ? record.items[0]?.name ?? "파일" : `${record.items[0]?.name ?? "파일"} 외 ${Math.max(0, record.items.length - 1)}개`) : record.kind === "url" ? "링크" : "텍스트 메시지";
    const meta = document.createElement("small");
    const peer = devices.find((device) => device.device_id === record.peer_id)?.display_name ?? record.peer_id;
    const when = new Date(record.created_at * 1000).toLocaleString(undefined, { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
    meta.textContent = `${record.direction === "send" || record.direction === "outbound" ? "보냄" : "받음"}  ${peer}  ${when}`;
    copy.append(title, meta);
    const badge = document.createElement("span");
    badge.className = "status-badge";
    badge.dataset.state = record.state;
    badge.textContent = stateLabel(record.state);
    open.append(recordIcon, copy, badge);
    open.addEventListener("click", () => { void act(() => showDetail(record.transfer_id)); });
    const remove = document.createElement("button");
    remove.className = "quiet delete-record";
    remove.title = "기록 삭제";
    remove.setAttribute("aria-label", `${title.textContent} 기록 삭제`);
    remove.append(icon("delete"));
    remove.addEventListener("click", () => { void act(async () => {
      await rpc("inbox.delete", { transfer_id: record.transfer_id });
      await refreshHistory();
    }); });
    row.append(open, remove);
    list.append(row);
  }
}

// Query by stable ID so notification clicks resolve the current persisted record.
async function showDetail(id: string): Promise<void> {
  const record = await rpc<TransferRecord>("inbox.get", { transfer_id: id });
  await invoke("show_window");
  currentDetail = record;
  element("detail-title").textContent = record.kind === "files" ? "파일 전송 내용" : record.kind === "url" ? "받은 링크" : "텍스트 내용";
  element("detail-meta").textContent = `${record.peer_id} · ${new Date(record.created_at * 1000).toLocaleString()} · ${stateLabel(record.state)}`;
  element("detail-body").textContent = record.body ?? "";
  element("detail-items").replaceChildren(...record.items.map((item) => {
    const line = document.createElement("p");
    line.textContent = `${item.saved_name ?? item.name} · ${stateLabel(item.state)}`;
    return line;
  }));
  element("copy-detail").hidden = record.kind === "files";
  element("open-detail").hidden = record.kind !== "url";
  element("reveal-detail").hidden = record.kind !== "files" || !record.items.some((item) => item.path);
  const dialog = element<HTMLDialogElement>("detail-dialog");
  if (!dialog.open) dialog.showModal();
  positionNotice();
}

// Refreshes are serialized so a slow response cannot overwrite a newer device selection state.
async function refresh(): Promise<void> {
  if (!desktop || refreshing) return;
  refreshing = true;
  try {
    const status = await rpc<JsonObject>("status");
    devices = await rpc<Device[]>("devices.list");
    connected = true;
    // Startup can outlast the initial IPC probe; remove its stale error only after a real successful response.
    if (element("notice").dataset.source === "connection") element("notice").hidden = true;
    networkReady = status.network_available !== false && status.network_reconfirmation_required !== true;
    if (!networkReady) showNotice("네트워크가 변경되었습니다. 설정 확인 후 서비스를 재시작하세요.", true, "connection");
    if (!selectedDevice()) selectedId = "";
    renderDevices();
    if (connected && isMain) void act(processPendingActivations);
    if (activePairSession) {
      try {
        const pairState = await rpc<JsonObject>("pairing.status", { session_id: activePairSession });
        renderPair(pairState);
        if (pairState.state === "paired") {
          activePairSession = "";
          showNotice("기기 등록이 완료되었어요.");
        }
      } catch (error) {
        activePairSession = "";
        showNotice(userError(error), true);
      }
    }
  } catch (error) {
    connected = false;
    networkReady = false;
    devices = [];
    renderDevices();
    showNotice(userError(error), true, "connection");
  } finally {
    refreshing = false;
    updateActions();
  }
}

// A bounded history query prevents the UI from pulling all received message bodies on every poll.
async function refreshHistory(): Promise<void> {
  records = await rpc<TransferRecord[]>("inbox.list", { limit: HISTORY_LIMIT });
  renderHistory();
}

// Auxiliary content is hosted by native windows; opening it never resizes or replaces the widget.
function navigate(destination: string): void {
  if (destination !== "inbox" && destination !== "settings") return;
  if (!desktop) { showNotice("별도 창은 데스크톱 앱에서 열 수 있습니다."); return; }
  void act(async () => { await invoke("open_utility_window", { kind: destination }); });
}

// Sending remains pending until the engine reports final storage; failure preserves the selection.
async function sendFiles(): Promise<void> {
  const target = selectedDevice();
  if (!target || sending) throw new Error("온라인 기기를 먼저 선택해주세요.");
  sending = true;
  beginProgress();
  updateActions();
  element("transfer-status").hidden = false;
  element("transfer-status").textContent = `${target.display_name}에 파일을 전송하고 있어요…`;
  try {
    const result = await invoke<TransferRecord>("send_files", { to: target.device_id, paths: selectedPaths, transferId: inFlightId });
    element("transfer-status").textContent = `파일 전송 ${stateLabel(result.state)}`;
    if (result.state === "completed") selectedPaths = [];
    else showNotice(transferFailureMessage(result), true);
    await refreshHistory();
  } finally {
    await finishProgress();
    sending = false;
    renderFiles();
    renderDevices();
  }
}

// A URL is recognized only when the entire input is one HTTP(S) address.
function messageKind(body: string): "text" | "url" {
  try {
    const value = new URL(body);
    return ["http:", "https:"].includes(value.protocol) && !/\s/.test(body) ? "url" : "text";
  } catch { return "text"; }
}

// Keep the user's draft until final success so an interrupted send does not lose their text.
async function sendMessage(): Promise<void> {
  const target = selectedDevice();
  const input = element<HTMLTextAreaElement>("message");
  if (!target || !input.value || sending) throw new Error("받는 기기와 내용을 확인해주세요.");
  sending = true;
  beginProgress();
  updateActions();
  try {
    const result = await invoke<TransferRecord>("send_message", { to: target.device_id, kind: messageKind(input.value), body: input.value, transferId: inFlightId });
    if (result.state === "completed") {
      input.value = "";
      showNotice(`${target.display_name}에 전달했어요.`);
    } else showNotice(transferFailureMessage(result), true);
    await refreshHistory();
  } finally { await finishProgress(); sending = false; updateActions(); }
}

// Partial and unknown outcomes must not encourage a blind retry that may duplicate received items.
function transferFailureMessage(record: TransferRecord): string {
  const reason = record.error ? ` ${userError(`${record.error}:`)}` : "";
  const duplicate = ["partial", "unknown"].includes(record.state)
    ? "이미 저장된 항목이 있을 수 있어요. 다시 보내기 전 수신 기록을 확인해주세요."
    : "기록에서 항목별 결과를 확인해주세요.";
  return `전송 ${stateLabel(record.state)}${reason}. ${duplicate}`;
}

// Caller-generated IDs make cancellation possible before the long-running send IPC returns.
function beginProgress(): void {
  inFlightId = crypto.randomUUID();
  element("cancel-transfer").hidden = false;
  element<HTMLButtonElement>("cancel-transfer").disabled = false;
  element("transfer-status").hidden = false;
  element("transfer-status").textContent = "전송 준비 중…";
  element("transfer-progress-bar").hidden = true;
  progressTimer = window.setInterval(() => { void updateProgress(); }, PROGRESS_INTERVAL_MS);
}

// Parse byte counters as integers so a large file never loses precision at the WebView boundary.
function byteCount(value: string | undefined): bigint {
  return value !== undefined && /^\d+$/.test(value) ? BigInt(value) : 0n;
}

// Human-readable sizes keep progress readable while all arithmetic remains integral.
function formatBytes(value: bigint): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let divisor = 1n;
  let index = 0;
  while (value / divisor >= 1024n && index < units.length - 1) { divisor *= 1024n; index += 1; }
  return `${(Number(value * 100n / divisor) / 100).toLocaleString(undefined, { maximumFractionDigits: index === 0 ? 0 : 1 })} ${units[index]}`;
}

// Display engine-reported byte counters; body transmission reaching 100% is distinct from final storage.
async function updateProgress(): Promise<void> {
  const id = inFlightId;
  if (!id) return;
  try {
    const record = await rpc<TransferRecord>("transfers.get", { transfer_id: id });
    if (inFlightId !== id) return;
    const count = record.items.filter((item) => item.state === "completed").length;
    const total = record.items.reduce((sum, item) => sum + byteCount(item.size), 0n);
    const transferred = record.items.reduce((sum, item) => sum + byteCount(item.bytes_transferred ?? (item.state === "completed" ? item.size : "0")), 0n);
    const bounded = transferred > total ? total : transferred;
    const progress = element<HTMLProgressElement>("transfer-progress-bar");
    progress.hidden = record.kind !== "files" || total === 0n;
    progress.value = total > 0n ? Number(bounded * 10000n / total) / 100 : 0;
    element("transfer-status").textContent = record.kind === "files"
      ? `${stateLabel(record.state)}  ${formatBytes(bounded)} / ${formatBytes(total)}  ·  ${count}/${record.items.length}개 저장`
      : `메시지 ${stateLabel(record.state)}`;
  } catch {
    // A transfer may not exist until source validation finishes; final send errors are shown separately.
  }
}

// Query persisted state even after a send error so partial success is not hidden by a rejected IPC call.
async function finishProgress(): Promise<void> {
  if (progressTimer !== undefined) window.clearInterval(progressTimer);
  await updateProgress();
  inFlightId = "";
  element("cancel-transfer").hidden = true;
}

// Settings write only known engine keys; the bridge never interprets arbitrary shell configuration.
async function loadSettings(): Promise<void> {
  const config = await rpc<JsonObject>("settings.get");
  element<HTMLInputElement>("setting-name").value = String(config.display_name ?? "");
  element<HTMLInputElement>("setting-folder").value = String(config.receive_dir ?? "");
  element<HTMLInputElement>("setting-listen").value = String(config.listen_addr ?? "");
  element<HTMLInputElement>("setting-networks").value = Array.isArray(config.allowed_networks) ? config.allowed_networks.join(", ") : "";
  element<HTMLInputElement>("setting-discovery").checked = config.discovery_enabled === true;
  const list = element("paired-devices");
  list.replaceChildren();
  for (const device of devices.filter((item) => item.paired)) {
    const row = document.createElement("div");
    row.className = "settings-row";
    const name = document.createElement("span");
    name.textContent = device.display_name;
    const button = document.createElement("button");
    button.className = "quiet";
    button.textContent = "등록 해제";
    button.addEventListener("click", () => { void act(async () => {
      await rpc("devices.unpair", { device_id: device.device_id });
      await refresh();
      await loadSettings();
    }); });
    row.append(name, button);
    list.append(row);
  }
}

// Show only the comparison and actionable connection data, not certificate or protocol internals.
function renderPair(result: JsonObject): void {
  element("pair-details").hidden = false;
  const states: Record<string, string> = { waiting: "상대 기기 연결 대기", compare: "양쪽 비교값을 확인하세요", paired: "등록 완료" };
  element("pair-state").textContent = result.cancelled ? "등록 취소됨" : states[String(result.state)] ?? "등록 상태 확인";
  const peer = typeof result.peer === "object" && result.peer !== null ? result.peer as JsonObject : undefined;
  element("pair-peer").textContent = typeof peer?.display_name === "string" ? peer.display_name : "";
  element("pair-address").textContent = typeof result.address === "string" ? `연결 주소  ${result.address}` : "";
  const comparison = typeof result.comparison === "string" ? result.comparison : "";
  element("pair-comparison").textContent = comparison ? comparison.match(/.{1,8}/g)?.join(" ") ?? comparison : "상대 기기에서 위 주소로 연결하면 비교값이 표시됩니다.";
  element("pair-expiry").textContent = typeof result.expires_at === "number" ? `${new Date(result.expires_at * 1000).toLocaleTimeString()}까지 확인할 수 있습니다.` : "";
  element<HTMLButtonElement>("pair-confirm").disabled = !comparison || result.state === "paired";
}

// The session ID stays internal once the actual pairing response has supplied it.
async function pair(method: string, params: JsonObject): Promise<void> {
  const result = await rpc<JsonObject>(method, params);
  renderPair(result);
  if (typeof result.session_id === "string") element<HTMLInputElement>("pair-session").value = result.session_id;
  if (typeof result.session_id === "string") activePairSession = result.session_id;
  if (method === "pairing.cancel") activePairSession = "";
  await refresh();
}

// Native authorization is separate from transfer acceptance, and unavailable development builds say so.
function renderNotificationPermission(permission: string): void {
  const labels: Record<string, string> = {
    granted: "알림이 허용되어 있습니다. 표시는 집중 모드 등 OS 설정에 따릅니다.",
    denied: "시스템 설정의 알림에서 FTA를 허용하세요. 수신은 계속됩니다.",
    prompt: "알림을 허용하면 새 수신 항목을 알려드립니다.",
    "os-managed": "알림 표시·차단은 Windows 알림 설정에서 관리합니다.",
    unavailable: "설치된 데스크톱 앱에서 알림을 사용할 수 있습니다.",
  };
  element("notification-status").textContent = labels[permission] ?? "알림 상태를 확인할 수 없습니다.";
  element<HTMLButtonElement>("notification-permission").disabled = ["granted", "os-managed", "unavailable"].includes(permission);
  element("notification-permission").textContent = permission === "granted" ? "허용됨" : permission === "os-managed" ? "OS에서 관리" : permission === "unavailable" ? "설치 앱 필요" : permission === "denied" ? "다시 확인" : "알림 허용";
}

// Only a direct user action can trigger the system's initial authorization prompt.
async function enableNotifications(): Promise<void> {
  renderNotificationPermission(await invoke<string>("notification_permission", { request: true }));
}

// Both warm and cold activations carry only a validated ID; contents are read from the persisted local inbox.
async function processPendingActivations(): Promise<void> {
  if (activationProcessing) return;
  activationProcessing = true;
  try {
    const ids = await invoke<string[]>("pending_activations");
    for (const id of ids) {
      try {
        await showDetail(id);
        await invoke("ack_activation", { transferId: id });
      } catch (error) {
        // A deleted record cannot be reopened, but a temporarily unavailable service should be retried.
        if (String(error).startsWith("NOT_FOUND:")) await invoke("ack_activation", { transferId: id });
        throw error;
      }
    }
  } finally { activationProcessing = false; }
}

// User confirmation accounts for active sends; closing the panel remains a non-destructive hide.
async function quit(): Promise<void> {
  if (!desktop) return;
  let active = sending;
  if (connected) {
    const current = await rpc<TransferRecord[]>("inbox.list", { limit: HISTORY_LIMIT });
    active ||= current.some((record) => ["sending", "receiving", "verifying", "committing"].includes(record.state));
  }
  if (active && !window.confirm("진행 중인 송수신이 중단될 수 있어요. 앱을 종료할까요?")) return;
  await invoke("exit_app");
}

// Bind each handler once; rerenders replace only dynamic lists and preserve drafts/focus.
for (const button of document.querySelectorAll<HTMLButtonElement>("[data-window]")) button.addEventListener("click", () => navigate(button.dataset.window ?? "send"));
for (const button of document.querySelectorAll<HTMLButtonElement>("[data-close-window]")) button.addEventListener("click", () => { void act(async () => { if (desktop) await invoke("close_utility_window"); }); });
for (const dialog of document.querySelectorAll<HTMLDialogElement>("dialog")) dialog.addEventListener("close", positionNotice);
on("add-device", "click", () => navigate("settings"));
on("refresh", "click", () => { void act(refresh); });
on("reload-inbox", "click", () => { void act(refreshHistory); });
on("message", "input", updateActions);
on("choose-files", "click", () => { void act(async () => { selectedPaths = await invoke<string[]>("choose_files"); renderFiles(); }); });
on("clear-files", "click", () => { selectedPaths = []; renderFiles(); });
on("send-files", "click", () => { void act(sendFiles); });
on("send-message", "click", () => { void act(sendMessage); });
on("cancel-transfer", "click", () => { void act(async () => {
  if (!inFlightId) return;
  await rpc("transfers.cancel", { transfer_id: inFlightId });
  element<HTMLButtonElement>("cancel-transfer").disabled = true;
  showNotice("취소를 요청했어요. 이미 저장된 항목은 보존됩니다.");
}); });
on("close-detail", "click", () => element<HTMLDialogElement>("detail-dialog").close());
on("copy-detail", "click", () => { void act(async () => { await invoke("copy_text", { value: currentDetail?.body ?? "" }); showNotice("복사했어요."); }); });
on("open-detail", "click", () => { void act(async () => { if (currentDetail?.body) await invoke("open_link", { value: currentDetail.body }); }); });
on("reveal-detail", "click", () => { void act(async () => { if (currentDetail) await invoke("reveal_transfer", { transferId: currentDetail.transfer_id }); }); });
on("quit", "click", () => { void act(quit); });
on("restart-engine", "click", () => { void act(async () => {
  if (sending) throw new Error("현재 전송이 끝난 뒤 엔진을 재시작해주세요.");
  await invoke("restart_engine");
  await refresh();
  showNotice("엔진을 재시작했어요.");
}); });
// Keep the DOM and draft alive while the native window changes size; only confirmed native state hides the surface.
function applyFolded(folded: boolean): void {
  if (!isMain) return;
  if (folded) element<HTMLDialogElement>("detail-dialog").close();
  document.body.classList.toggle("folded", folded);
  element("expand-window").hidden = !folded;
  dragPoint = undefined;
  highlightDropTarget();
}
async function setFolded(folded: boolean): Promise<void> {
  if (desktop) applyFolded(await invoke<boolean>("set_folded", { folded }));
}
on("fold-window", "click", () => { void act(() => setFolded(true)); });
on("close-app-window", "click", () => { void act(async () => { if (desktop) await invoke("close_app_window"); }); });

// Native dragging starts only on non-interactive titlebar space, so window actions remain ordinary buttons.
for (const handle of document.querySelectorAll<HTMLElement>("[data-drag-handle]")) {
  handle.addEventListener("pointerdown", (event) => {
    if (!desktop || event.button !== 0 || (event.target as HTMLElement).closest("button, input, textarea, a")) return;
    event.preventDefault();
    void act(async () => { await invoke("start_window_drag"); });
  });
}

// The folded button is the only visible surface; distinguish a deliberate move from a click to expand.
const WIDGET_DRAG_THRESHOLD_PX = 4;
let widgetPointer: { id: number; x: number; y: number } | undefined;
let widgetDragged = false;
on("expand-window", "pointerdown", (raw) => {
  const event = raw as PointerEvent;
  if (event.button !== 0) return;
  widgetDragged = false;
  widgetPointer = { id: event.pointerId, x: event.clientX, y: event.clientY };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
});
on("expand-window", "pointermove", (raw) => {
  const event = raw as PointerEvent;
  if (!desktop || !widgetPointer || widgetDragged || !event.buttons) return;
  if (Math.hypot(event.clientX - widgetPointer.x, event.clientY - widgetPointer.y) < WIDGET_DRAG_THRESHOLD_PX) return;
  widgetDragged = true;
  widgetPointer = undefined;
  const button = event.currentTarget as HTMLElement;
  if (button.hasPointerCapture(event.pointerId)) button.releasePointerCapture(event.pointerId);
  void act(async () => { await invoke("start_window_drag"); });
});
on("expand-window", "pointerup", (raw) => {
  const event = raw as PointerEvent;
  // Some native move paths coalesce pointermove; the release must still reject a moved gesture.
  if (widgetPointer && Math.hypot(event.clientX - widgetPointer.x, event.clientY - widgetPointer.y) >= WIDGET_DRAG_THRESHOLD_PX) widgetDragged = true;
  widgetPointer = undefined;
});
on("expand-window", "pointercancel", () => { widgetPointer = undefined; });
on("expand-window", "click", (event) => {
  // Native dragging can synthesize detail=0 clicks too; click count does not prove keyboard activation.
  if (widgetDragged) { event.preventDefault(); return; }
  void act(() => setFolded(false));
});
on("expand-window", "keydown", (raw) => {
  const event = raw as KeyboardEvent;
  if (event.key !== "Enter" && event.key !== " ") return;
  event.preventDefault();
  if (event.repeat || event.isComposing) return;
  widgetDragged = false;
  widgetPointer = undefined;
  void act(() => setFolded(false));
});
on("expand-window", "keyup", (raw) => {
  // Space releases must not generate another implicit button click after explicit keyboard expansion.
  const event = raw as KeyboardEvent;
  if (event.key === " ") event.preventDefault();
});
on("pin", "click", () => { void act(async () => {
  const pinned = element("pin").getAttribute("aria-pressed") !== "true";
  await invoke("set_pinned", { pinned });
  element("pin").setAttribute("aria-pressed", String(pinned));
  localStorage.setItem(PINNED_KEY, String(pinned));
}); });
on("autostart", "click", () => { void act(async () => {
  await invoke("set_autostart", { enabled: !autostartEnabled });
  autostartEnabled = !autostartEnabled;
  element("autostart").textContent = autostartEnabled ? "켜짐 · 끄기" : "꺼짐 · 켜기";
}); });
on("notification-permission", "click", () => { void act(enableNotifications); });
on("pair-open", "click", () => { void act(() => pair("pairing.open", {})); });
on("pair-connect", "click", () => { void act(() => pair("pairing.connect", { target: element<HTMLInputElement>("pair-target").value.trim() })); });
on("pair-confirm", "click", () => { void act(() => pair("pairing.confirm", { session_id: element<HTMLInputElement>("pair-session").value.trim() })); });
on("pair-cancel", "click", () => { void act(() => pair("pairing.cancel", { session_id: element<HTMLInputElement>("pair-session").value.trim() })); });
on("settings-form", "submit", (event) => {
  event.preventDefault();
  void act(async () => {
    const values: JsonObject = {
      display_name: element<HTMLInputElement>("setting-name").value.trim(),
      receive_dir: element<HTMLInputElement>("setting-folder").value,
      listen_addr: element<HTMLInputElement>("setting-listen").value.trim(),
      allowed_networks: element<HTMLInputElement>("setting-networks").value.split(",").map((value) => value.trim()).filter(Boolean),
      discovery_enabled: element<HTMLInputElement>("setting-discovery").checked,
    };
    await rpc("settings.update", { values });
    showNotice("설정을 저장했어요. 네트워크 설정은 엔진 재시작 후 적용됩니다.");
    await refresh();
  });
});

// Settings-only OS controls are initialized in their own window without starting an engine or receipt loop.
async function initializeSettingsControls(): Promise<void> {
  try {
    autostartEnabled = await invoke<boolean>("autostart_status");
    element("autostart").textContent = autostartEnabled ? "켜짐 · 끄기" : "꺼짐 · 켜기";
  } catch (error) {
    element("autostart").textContent = "상태 확인 불가";
    element<HTMLButtonElement>("autostart").disabled = true;
    showNotice(userError(error), true);
  }
  await act(async () => {
    renderNotificationPermission(await invoke<string>("notification_permission", { request: false }));
  });
}

let settingsLoaded = false;
// Retry initial IPC readiness without replacing a settings draft on each subsequent status refresh.
async function refreshUtility(): Promise<void> {
  await refresh();
  if (!connected) return;
  if (view === "inbox") await refreshHistory();
  else if (!settingsLoaded) { await loadSettings(); settingsLoaded = true; }
}

// In a browser there is no native bridge: controls stay disabled and no fake devices are inserted.
async function initialize(): Promise<void> {
  renderDevices();
  renderHistory();
  if (!desktop) {
    showNotice("미리보기 · 전송 불가");
    for (const button of document.querySelectorAll<HTMLButtonElement>("#settings-panel button:not([data-close-window]), #pin, #fold-window, #expand-window, #close-app-window")) button.disabled = true;
    return;
  }
  if (!isMain) {
    window.setInterval(() => { void act(refreshUtility); }, REFRESH_INTERVAL_MS);
    await act(refreshUtility);
    if (view === "settings") await act(initializeSettingsControls);
    return;
  }
  await listen<boolean>("fta:fold-state", (event) => applyFolded(event.payload));
  applyFolded(await invoke<boolean>("fold_state"));
  await listen<string>("fta:navigate", (event) => navigate(event.payload));
  await listen("fta:quit-requested", () => { void act(quit); });
  await listen("fta:panel-hidden", () => { selectedPaths = []; renderFiles(); });
  await listen<string>("fta:drop-error", (event) => {
    // Rejected paths must clear hover feedback just like a completed or cancelled native drop.
    dragPoint = undefined;
    highlightDropTarget();
    showNotice(event.payload, true);
  });
  await listen<DropPoint>("fta:drag-position", (event) => {
    dragPoint = event.payload;
    highlightDropTarget();
  });
  await listen("fta:drag-left", () => { dragPoint = undefined; highlightDropTarget(); });
  await listen<{ paths: string[]; position: DropPoint }>("fta:files-dropped", (event) => {
    // Hit-test the actual drop, never fall back to a previously selected computer.
    const target = dropTarget(event.payload.position);
    dragPoint = undefined;
    highlightDropTarget();
    if (sending) { showNotice("현재 전송이 끝난 뒤 다시 파일을 놓아주세요.", true); return; }
    selectedPaths = [];
    if (!target) { renderFiles(); showNotice("온라인 기기 아이콘에 파일을 놓아주세요.", true); return; }
    selectedId = target.device_id;
    localStorage.setItem(LAST_DEVICE_KEY, selectedId);
    selectedPaths = event.payload.paths;
    renderFiles();
    renderDevices();
    void act(sendFiles);
  });
  await listen("fta:activation-available", () => { void act(processPendingActivations); });
  await listen<ReceiptEvent>("fta:receipt", (event) => { void act(async () => {
    const receipt = event.payload;
    if (receiptInFlight.has(receipt.sequence)) return;
    receiptInFlight.add(receipt.sequence);
    try {
      await refreshHistory();
      if (receipt.record.direction === "receive" && ["completed", "partial"].includes(receipt.record.state)
          && ["granted", "os-managed"].includes(await invoke<string>("notification_permission", { request: false }))) {
        await invoke("notify_receipt", { sequence: receipt.sequence, transferId: receipt.transfer_id });
      }
      // Disabled OS notifications still acknowledge the persisted record; permission is not receipt approval.
      await invoke("ack_receipt", { sequence: receipt.sequence });
    } finally {
      receiptInFlight.delete(receipt.sequence);
    }
  }); });
  await invoke("notifications_ready");
  if (localStorage.getItem(PINNED_KEY) === "true") {
    await act(async () => {
      await invoke("set_pinned", { pinned: true });
      element("pin").setAttribute("aria-pressed", "true");
    });
  }
  try { await invoke("start_engine"); } catch (error) { showNotice(userError(error), true, "connection"); }
  window.setInterval(() => { void refresh(); }, REFRESH_INTERVAL_MS);
  await refresh();
  if (connected) await refreshHistory();
  await processPendingActivations();
}

void act(initialize);
