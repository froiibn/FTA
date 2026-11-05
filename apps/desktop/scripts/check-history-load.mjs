import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import vm from "node:vm";
import { test } from "node:test";

// Run the actual refresh functions without a native window so failed discovery and
// delayed IPC can be exercised deterministically, without inventing online peers.
const source = stripTypeScriptTypes(readFileSync(new URL("../src/main.ts", import.meta.url), "utf8"));
function declaration(name) {
  const start = source.indexOf(`async function ${name}(`);
  assert.notEqual(start, -1);
  const end = source.indexOf("\n}", start);
  return source.slice(start, end + 2);
}
function fixture(rpc) {
  const context = vm.createContext({
    view: "inbox", records: [], devices: [], HISTORY_LIMIT: 100, settingsLoaded: false,
    rpc, renders: [], failures: [], recoveredSources: [],
    clearPassiveNotice(source) { context.recoveredSources.push(source); },
    renderHistory() { context.renders.push([...context.records]); },
    refresh() { throw new Error("History must not depend on status readiness"); },
    userError: String, showNotice(error) { context.failures.push(error); },
  });
  vm.runInContext(`${declaration("refreshHistory")}\n${declaration("refreshUtility")}`, context);
  return context;
}

test("first opening renders saved records before a stalled device lookup", async () => {
  let finishDiscovery;
  const c = fixture(async (method) => {
    if (method === "inbox.list") return [{ transfer_id: "persisted-receipt" }];
    return new Promise((resolve) => { finishDiscovery = resolve; });
  });
  const pending = c.refreshUtility();
  await new Promise(setImmediate);
  assert.equal(c.renders[0][0].transfer_id, "persisted-receipt");
  finishDiscovery([]);
  await pending;
  assert.equal(c.failures.length, 0);
});

test("failed discovery preserves the loaded list without a manual refresh", async () => {
  const c = fixture(async (method) => {
    if (method === "inbox.list") return [{ transfer_id: "saved-offline" }];
    throw new Error("Discovery unavailable");
  });
  await c.refreshUtility();
  assert.equal(c.renders[0][0].transfer_id, "saved-offline");
  assert.equal(c.failures.length, 0);
});

test("a failed initial history query is retried by the same passive refresh", async () => {
  let ready = false;
  const c = fixture(async (method) => {
    if (!ready) throw new Error("Engine starting");
    return method === "inbox.list" ? [{ transfer_id: "recovered" }] : [];
  });
  await c.refreshUtility();
  assert.equal(c.failures.length, 1);
  ready = true;
  await c.refreshUtility();
  assert.equal(c.renders[0][0].transfer_id, "recovered");
  assert.ok(c.recoveredSources.includes("utility-refresh"));
});

test("reopening refreshes new persisted records and resolves peer labels afterwards", async () => {
  let generation = 0;
  const c = fixture(async (method) => method === "inbox.list"
    ? [{ transfer_id: `receipt-${++generation}` }] : [{ device_id: "peer", display_name: "Saved peer" }]);
  await c.refreshUtility();
  await c.refreshUtility();
  assert.equal(c.records[0].transfer_id, "receipt-2");
  assert.equal(c.devices[0].display_name, "Saved peer");
});
