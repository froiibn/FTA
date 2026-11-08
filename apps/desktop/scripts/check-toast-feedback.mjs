import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import vm from "node:vm";
import { test } from "node:test";

// Execute the shipped feedback functions with a controlled clock: real-time sleeps
// cannot reliably catch stale timers or periodic errors that keep a toast visible.
const source = stripTypeScriptTypes(readFileSync(new URL("../src/main.ts", import.meta.url), "utf8"));
function declaration(name) {
  const match = new RegExp(`(?:async )?function ${name}\\(`).exec(source);
  assert.ok(match, name);
  return source.slice(match.index, source.indexOf("\n}", match.index) + 2);
}
function fixture(view = "main") {
  let now = 0;
  let nextTimer = 0;
  const timers = new Map();
  const classes = new Set();
  const bodyClasses = new Set();
  const toast = { hidden: true, style: { setProperty() {} }, classList: {
    toggle(name, on) { if (on) classes.add(name); else classes.delete(name); },
  } };
  const progress = { hidden: false };
  const progressElements = {
    "transfer-progress-bar": progress, "transfer-status": { hidden: false }, "cancel-transfer": { hidden: false },
    "expand-window": { hidden: true },
  };
  const context = vm.createContext({
    view, isMain: view === "main", toast, toastMessage: {}, toastTimer: undefined,
    passiveNotices: new Map(), settingsActionCount: 0, settingsActionResult: undefined,
    document: { getElementById: () => null, body: { classList: {
      contains: (name) => bodyClasses.has(name),
      toggle(name, on) { if (on) bodyClasses.add(name); else bodyClasses.delete(name); },
    } } },
    window: {
      setTimeout(fn, delay) { timers.set(++nextTimer, { fn, due: now + delay }); return nextTimer; },
      clearTimeout(id) { timers.delete(id); }, clearInterval() {},
    },
    setSettingsWorking() {}, element: (id) => progressElements[id],
    progressTimer: undefined, progressDismissTimer: undefined, inFlightId: "test-transfer",
    crypto: { randomUUID: () => "new-transfer" }, stateLabel: (state) => state,
    dragPoint: undefined, highlightDropTarget() {},
  });
  const names = ["showToast", "hideToast", "showNotice", "clearPassiveNotice", "rememberSettingsResult",
    "beginSettingsAction", "finishSettingsAction", "beginProgress", "finishProgress", "hideTransferProgress",
    "updateProgress", "renderTransferProgress", "byteCount", "formatBytes", "applyFolded"];
  const constants = source.match(/const (?:TOAST_DURATION_MS|COMPLETED_FILE_PROGRESS_MS|PROGRESS_INTERVAL_MS) = [\d_]+;/g);
  vm.runInContext(`${constants.join("\n")}\n${names.map(declaration).join("\n")}`, context);
  return { context, toast, classes, progress, progressElements, timers,
    advance(ms) {
      now += ms;
      for (const [id, timer] of [...timers]) if (timer.due <= now) { timers.delete(id); timer.fn(); }
    },
  };
}

for (const view of ["main", "inbox", "settings", "detail"]) {
  test(`${view}: success and error feedback expire after one second`, () => {
    const f = fixture(view);
    for (const error of [false, true]) {
      f.context.showNotice("Actual action result", error);
      assert.equal(f.toast.hidden, false);
      assert.equal(f.classes.has("error"), error);
      f.advance(999);
      assert.equal(f.toast.hidden, false);
      f.advance(1);
      assert.equal(f.toast.hidden, true);
    }
  });
}

test("a new action gets its own full lifetime and cannot be hidden by an older timer", () => {
  const f = fixture();
  f.context.showNotice("First"); f.advance(700);
  f.context.showNotice("Second"); f.advance(300);
  assert.equal(f.toast.hidden, false);
  assert.equal(f.context.toastMessage.textContent, "Second");
  f.advance(700);
  assert.equal(f.toast.hidden, true);
});

test("identical passive failures do not replay, but recovery rearms a later failure", () => {
  const f = fixture();
  f.context.showNotice("Unavailable", true, "connection");
  f.advance(1000);
  for (let i = 0; i < 4; ++i) {
    f.context.showNotice("Unavailable", true, "connection"); f.advance(3000);
    assert.equal(f.toast.hidden, true);
  }
  f.context.clearPassiveNotice("connection");
  f.context.showNotice("Unavailable", true, "connection");
  assert.equal(f.toast.hidden, false);
  f.advance(1000);
  assert.equal(f.toast.hidden, true);
});

test("new passive errors remain visible and recovery cannot dismiss an unrelated action", () => {
  const f = fixture();
  f.context.showNotice("First error", true, "connection"); f.advance(1000);
  f.context.showNotice("Different error", true, "connection");
  assert.equal(f.toast.hidden, false);
  f.context.showNotice("Copied");
  f.context.clearPassiveNotice("connection");
  assert.equal(f.context.toastMessage.textContent, "Copied");
  assert.equal(f.toast.hidden, false);
});

test("nested settings batches defer feedback and keep an earlier error over cleanup success", () => {
  const f = fixture("settings");
  f.context.beginSettingsAction(); f.context.beginSettingsAction();
  f.context.showNotice("Save failed", true);
  f.context.showNotice("Cleanup complete");
  f.context.finishSettingsAction();
  assert.equal(f.toast.hidden, true);
  f.context.finishSettingsAction();
  assert.equal(f.context.toastMessage.textContent, "Save failed");
  assert.equal(f.classes.has("error"), true);
  f.advance(1000);
  assert.equal(f.toast.hidden, true);
});

test("failed or cancelled transfers clear progress immediately", () => {
  const f = fixture();
  f.context.finishProgress();
  assert.equal(f.progress.hidden, true);
  assert.equal(f.context.inFlightId, "");
});

// Simulate a poll returning after final storage to catch the race with the dismissal timer.
test("confirmed file progress shows 100% for three seconds and ignores a late poll", async () => {
  const f = fixture();
  let reply;
  f.context.rpc = () => new Promise((resolve) => { reply = resolve; });
  const poll = f.context.updateProgress();
  f.context.finishProgress({ kind: "files", state: "completed", items: [{ state: "completed", size: "0" }] });
  assert.equal(f.progress.value, 100);
  assert.equal(f.progressElements["cancel-transfer"].hidden, true);
  f.advance(2999);
  assert.equal(f.progress.hidden, false);
  f.advance(1);
  reply({ kind: "files", state: "sending", items: [{ size: "100", bytes_transferred: "30" }] });
  await poll;
  assert.equal(f.progress.hidden, true);
  assert.equal(f.progressElements["transfer-status"].hidden, true);
});

test("starting another transfer cancels the previous completion timer", () => {
  const f = fixture();
  f.context.window.setInterval = () => 123;
  f.context.finishProgress({ kind: "files", state: "completed", items: [] });
  f.context.beginProgress({ paired: true });
  f.context.renderTransferProgress({ kind: "files", state: "sending", items: [{ size: "100", bytes_transferred: "30" }] });
  f.advance(3000);
  assert.equal(f.progress.hidden, false);
  assert.equal(f.progress.value, 30);
});

// Both an already-visible result and a newly arriving result must stay off the
// folded widget, without replaying a stale toast after the user expands it.
test("folding dismisses an active toast and cancels its timer", () => {
  const f = fixture();
  f.context.showNotice("Action completed");
  f.context.applyFolded(true);
  assert.equal(f.toast.hidden, true);
  assert.equal(f.timers.size, 0);
  f.context.applyFolded(false);
  assert.equal(f.toast.hidden, true);
  f.context.showNotice("New action");
  assert.equal(f.toast.hidden, false);
  f.advance(1000);
  assert.equal(f.toast.hidden, true);
});

test("results arriving while folded never create a visible toast or timer", () => {
  const f = fixture();
  f.context.applyFolded(true);
  f.context.showNotice("Connection unavailable", true, "connection");
  f.context.showNotice("Another action");
  assert.equal(f.toast.hidden, true);
  assert.equal(f.timers.size, 0);
  f.context.applyFolded(false);
  assert.equal(f.toast.hidden, true);
});
