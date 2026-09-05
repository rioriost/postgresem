/* Optional browser regression: requires Playwright, never connects to PostgreSQL. */
"use strict";

const assert = require("node:assert/strict");
const {createServer} = require("node:http");
const {readFileSync} = require("node:fs");
const {join} = require("node:path");
const {chromium} = require("playwright");

const scenarios = [
  {id: "recognized-revenue", title: "Recognized revenue", question: "What is recognized revenue?",
    pitfall: "Counting pending orders.", business_rule: "Recognize paid orders only."},
  {id: "sku-fanout", title: "SKU-RED revenue", question: "What is revenue of orders containing SKU-RED?",
    pitfall: "Counting an order twice.", business_rule: "Count each order once."},
  {id: "active-mrr", title: "Active MRR", question: "What is current MRR?",
    pitfall: "Counting inactive contracts.", business_rule: "Count active contracts only."},
];
const revision = "test-revision";
const query = (value) => ({
  columns: [{name: "revenue", type: "numeric"}], rows: [[value]],
  query_id: "test-query-id", semantic_revision: revision, truncated: false,
});
const bootstrap = {
  schema_version: "1", title: "Test lab", scenarios, semantic_revision: revision,
  models: [{name: "orders", field_count: 10, metric_count: 4}],
  contract: {raw_sql: false}, planner: {enabled: true, model: "test-model"},
  disclaimer: "Original server disclaimer.",
};
function compared(payload) {
  const scenario = scenarios.find((item) => item.id === payload.scenario);
  return {
    schema_version: "1", scenario, mode: payload.mode, question: scenario.question,
    baseline: {correct: false, value: "545.50", sql: "SELECT sum(amount) FROM commerce.orders",
      role: "postgresem_analyst", choice: "A", explanation: "Original planner reasoning."},
    semantic: {correct: true, value: "200.50", choice: "A", lsq: {model: "orders"},
      validation: {valid: true}, explanation: {lineage: "orders"},
      result: query("200.50"), catalog: {semantic_revision: revision}},
    expected: {value: "200.50", derivation: "120.00 + 80.50 (each qualifying order once)", contributing_rows: []},
    source: {fingerprint: "test-ledger", orders: [{external_id: "<img src=x onerror=alert(1)>", amount: "120.00"}],
      items: [], subscriptions: []},
    comparison: {same_role: true, stable_source: true, verdict: "Original server verdict."},
    planner: payload.mode === "planner"
      ? {model: "test-model", baseline_reason: "Original SQL reason.", semantic_reason: "Original metric reason."} : null,
  };
}
const receipt = {mutation_id: "test-mutation-id", affected_rows: 1, replayed: false, semantic_revision: revision};
const ingested = {
  schema_version: "1", before: query("200.50"), after: query("245.50"),
  validation: {valid: true}, first: receipt, replay: {...receipt, replayed: true},
  reconciliation: {state: {status: "committed", mutation_id: receipt.mutation_id}},
  conflicting_retry: {rejected: true, code: "MUTATION_IDEMPOTENCY_CONFLICT"},
  actual_delta: "45.00", expected_delta: "45.00", consistent: true,
};
const guarded = {
  schema_version: "1", passed: true,
  rejections: [{name: "Raw SQL", code: "LSQ_INVALID_JSON", rejected: true}],
  tenants: [{name: "Tenant A", role: "postgresem_tenant_a", direct_value: "250.00",
    semantic_value: "250.00", query_id: "test-tenant-query", rls_enforced: true}],
};

async function main() {
  const files = {"/": ["index.html", "text/html"], "/app.js": ["app.js", "text/javascript"],
    "/styles.css": ["styles.css", "text/css"]};
  const server = createServer((request, response) => {
    const file = files[request.url];
    if (!file) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, {"Content-Type": file[1], "Cache-Control": "no-store",
      "Content-Security-Policy": "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'"});
    response.end(readFileSync(join(__dirname, "static", file[0])));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  let browser;
  try {
    browser = await chromium.launch();
    const page = await browser.newPage({locale: "ja-JP"});
    const calls = [];
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    let delayComparison = null;
    let failComparison = false;
    await page.route("**/api/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      const payload = route.request().postDataJSON();
      calls.push({path, payload});
      if (path === "/api/compare" && delayComparison) await delayComparison;
      const value = path === "/api/bootstrap" ? bootstrap : path === "/api/compare" ? compared(payload)
        : path === "/api/ingest" ? ingested : path === "/api/guards" ? guarded : null;
      assert.notEqual(value, null, "unexpected API request");
      await route.fulfill({status: failComparison && path === "/api/compare" ? 409 : 200,
        contentType: "application/json",
        body: JSON.stringify(failComparison && path === "/api/compare"
          ? {error: {code: "DEMO_SOURCE_CHANGED", message: "Original diagnostic."}} : value)});
    });
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.waitForFunction(() => !document.querySelector("#compare-button").disabled);
    assert.equal(await page.locator("html").getAttribute("lang"), "en", "browser locale must not select Japanese");
    assert.equal(await page.locator("#language-select").inputValue(), "en");
    assert.equal(await page.locator("#compare-heading").innerText(), "Compare meaning");
    assert.match(await page.locator("#activity").innerText(), /Ready/);
    assert.doesNotMatch(await page.locator("main").innerText(), /[\u3040-\u30ff\u3400-\u9fff]/u);
    assert.equal(calls.length, 1);

    await page.locator('input[name="scenario"][value="sku-fanout"]').check();
    await page.locator("#mode-planner").check();
    await page.locator("#compare-button").click();
    await page.waitForFunction(() => !document.querySelector("#compare-result").hidden);
    assert.equal(await page.locator("#compare-button").isEnabled(), true);
    const originalSql = await page.locator(".sql-trace code").innerText();
    const source = page.locator("#compare-result .evidence").last();
    await source.locator(":scope > summary").click();
    const beforeSwitch = calls.length;
    await page.locator("#language-select").selectOption("ja");
    assert.equal(await page.locator("html").getAttribute("lang"), "ja");
    assert.equal(await page.locator("#compare-heading").innerText(), "意味を比較する");
    assert.match(await page.locator("#compare-result").innerText(), /SKU と注文の粒度/);
    assert.equal(await page.locator('input[name="scenario"]:checked').inputValue(), "sku-fanout");
    assert.equal(await page.locator('input[name="mode"]:checked').inputValue(), "planner");
    assert.equal(await source.evaluate((node) => node.open), true);
    assert.equal(await page.locator(".sql-trace code").innerText(), originalSql);
    assert.match(await page.locator("#compare-result").innerText(), /test-query-id/);
    assert.match(await page.locator("#compare-result").innerText(), /200\.50/);
    assert.match(await page.locator("#compare-result").innerText(), /Original SQL reason/);
    assert.equal(await page.locator("#compare-result img").count(), 0);
    await page.locator("#language-select").selectOption("en");
    assert.equal(calls.length, beforeSwitch, "switching must not perform queries or writes");
    assert.equal(await page.locator("#compare-result").getAttribute("aria-label"), "Comparison results");
    assert.equal(await page.locator("#revision").innerText(), revision);

    for (const language of ["en", "ja"]) {
      await page.locator("#language-select").selectOption(language);
      const dialog = page.waitForEvent("dialog");
      const click = page.locator("#ingest-button").click();
      const confirmation = await dialog;
      assert.match(confirmation.message(), language === "en" ? /Changes persist/ : /変更は永続化/);
      await confirmation.dismiss();
      await click;
      assert.equal(calls.length, beforeSwitch, "cancelling must not write");
    }
    await page.locator("#language-select").selectOption("en");
    assert.match(await page.locator("#activity").innerText(), /Write cancelled/);

    page.once("dialog", (dialog) => dialog.accept());
    await page.locator("#ingest-button").click();
    await page.waitForFunction(() => !document.querySelector("#ingest-result").hidden);
    await page.locator("#guards-button").click();
    await page.waitForFunction(() => !document.querySelector("#guards-result").hidden);
    const afterWrite = calls.length;
    await page.locator("#language-select").selectOption("ja");
    assert.match(await page.locator("#ingest-result").innerText(), /保存と再実行の記録/);
    assert.match(await page.locator("#guards-result").innerText(), /拒否と固定テナント/);
    assert.match(await page.locator(".stale-notice").innerText(), /書き込み前/);
    await page.locator("#language-select").selectOption("en");
    assert.match(await page.locator("#ingest-result").innerText(), /245\.50/);
    assert.match(await page.locator("#guards-result").innerText(), /LSQ_INVALID_JSON/);
    assert.doesNotMatch(await page.locator("main").innerText(), /[\u3040-\u30ff\u3400-\u9fff]/u);
    assert.equal(calls.length, afterWrite);

    let release;
    delayComparison = new Promise((resolve) => { release = resolve; });
    await page.locator("#compare-button").click();
    await page.waitForFunction(() => document.querySelector("#compare-button").disabled);
    await page.locator("#language-select").selectOption("ja");
    assert.match(await page.locator("#compare-empty").innerText(), /比較を実行中/);
    assert.equal(await page.locator("#ingest-button").isDisabled(), true);
    release();
    await page.waitForFunction(() => !document.querySelector("#compare-result").hidden);
    assert.match(await page.locator("#compare-result").innerText(), /期待値と一致/);
    assert.equal(await page.locator(".stale-notice").count(), 0);
    delayComparison = null;

    failComparison = true;
    await page.locator("#compare-button").click();
    await page.waitForFunction(() => !document.querySelector("#error-panel").hidden);
    const afterError = calls.length;
    assert.match(await page.locator("#error-title").innerText(), /比較に失敗/);
    await page.locator("#language-select").selectOption("en");
    assert.equal(await page.locator("#error-title").innerText(), "Comparison failed");
    assert.match(await page.locator("#error-message").innerText(), /DEMO_SOURCE_CHANGED/);
    assert.match(await page.locator("#error-note").innerText(), /No substitute results/);
    assert.match(await page.locator("#compare-empty").innerText(), /Could not fetch/);
    assert.equal(calls.length, afterError);

    await page.setViewportSize({width: 375, height: 812});
    assert.equal(await page.locator("#language-select").isVisible(), true);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true, "mobile layout overflow");
    await page.locator("#language-select").evaluate((select) => {
      select.add(new Option("Unsupported", "unsupported"));
      select.value = "unsupported";
      select.dispatchEvent(new Event("change", {bubbles: true}));
    });
    assert.equal(await page.locator("html").getAttribute("lang"), "en");
    assert.equal(await page.locator("#language-select").inputValue(), "en");
    assert.equal(await page.locator("#error-title").innerText(), "Language switch failed");
    assert.equal(calls.length, afterError, "unsupported languages must not trigger API calls");
    await page.reload();
    await page.waitForFunction(() => !document.querySelector("#compare-button").disabled);
    assert.equal(await page.locator("html").getAttribute("lang"), "en");
    assert.equal(calls.filter((call) => call.path === "/api/ingest").length, 1);
    assert.deepEqual(errors, []);
    console.log("PASS: bilingual UI, defaults, cached results, in-flight switching, errors, and no repeated writes");
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
