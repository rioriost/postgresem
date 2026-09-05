"use strict";

(() => {
  const $ = (selector) => document.querySelector(selector);
  const state = {
    language: "en", ready: false, busy: false, bootstrap: null, scenario: null,
    connection: "connecting", activity: null, error: null,
    results: {}, workflows: {}, comparisonStale: false,
  };
  const t = (english, japanese) => state.language === "ja" ? japanese : english;
  const staticText = [...document.querySelectorAll("[data-ja], [data-ja-label], [data-ja-content]")].map((node) => ({
    node, text: node.textContent, label: node.getAttribute("aria-label"), content: node.getAttribute("content"),
  }));
  const workflowLabels = {
    compare: ["Comparison", "比較"],
    ingest: ["Write and idempotency", "書き込みと冪等性の検証"],
    guards: ["Rejection and tenant isolation", "拒否とテナント分離の検証"],
  };
  const scenarioCopy = {
    "recognized-revenue": {
      title: "確定した注文の売上",
      category: "PAID REVENUE",
      question: "この台帳で、売上として認識する注文金額はいくら？",
      pitfall: "amount を単純合計すると、未確定・キャンセルの注文も含まれます。",
      rule: "status = paid の注文だけ、注文金額の全額を売上に数えます。",
    },
    "sku-fanout": {
      title: "SKU と注文の粒度",
      category: "FANOUT / SKU-RED",
      question: "SKU-RED を含む確定注文の売上は？ 1注文は1回だけ数えます。",
      pitfall: "明細を JOIN すると、複数の該当明細で注文金額が重複します。",
      rule: "SKU-RED を含む paid 注文の全額を order_id ごとに一度だけ集計。SKU 別の配賦売上ではありません。",
    },
    "active-mrr": {
      title: "有効な契約の MRR",
      category: "ACTIVE MRR",
      question: "デモ契約 1・2・3 の、現在の月次経常収益はいくら？",
      pitfall: "解約後も残る契約行まで合計すると、現在の MRR を過大に数えます。",
      rule: "active な契約の monthly_amount だけを月次経常収益に数えます。",
    },
  };

  function scenarioText(scenario, field) {
    const copy = state.language === "ja" ? scenarioCopy[scenario.id] : null;
    return copy?.[field === "business_rule" ? "rule" : field] || scenario[field];
  }

  class UiError extends Error {
    constructor(english, japanese) {
      super(english);
      this.japanese = japanese;
    }

    localizedMessage() {
      return t(this.message, this.japanese);
    }
  }

  function element(tag, value, className) {
    const node = document.createElement(tag);
    if (value !== undefined) node.textContent = value;
    if (className) node.className = className;
    return node;
  }

  function display(value) {
    if (value === undefined) return t("Not fetched", "未取得");
    if (value === null) return "null";
    return typeof value === "object" ? JSON.stringify(value) : String(value);
  }

  function record(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value);
  }

  function requireResponse(condition, english, japanese) {
    if (!condition) throw new UiError(`DEMO_RESPONSE_INVALID: ${english}`, `DEMO_RESPONSE_INVALID: ${japanese}`);
  }

  function decimal(value) {
    requireResponse(typeof value === "string" && /^-?\d+(?:\.\d+)?$/.test(value),
      "Expected a decimal string. The result was not converted or substituted.",
      "十進数の文字列が必要です。結果を数値に変換・補完せず停止しました。");
    return value;
  }

  function badge(passed, passText, failText) {
    if (passed === true) return element("span", `✓ ${passText}`, "badge success");
    if (passed === false) return element("span", `! ${failText}`, "badge failure");
    return element("span", t("Not confirmed", "未確認"), "badge neutral");
  }

  function facts(entries, className = "") {
    const list = element("dl", undefined, `facts ${className}`);
    for (const [label, value] of entries) {
      const row = element("div");
      row.append(element("dt", label), element("dd", display(value)));
      list.append(row);
    }
    return list;
  }

  function trace(title, value) {
    const details = element("details", undefined, "trace");
    const pre = element("pre");
    pre.tabIndex = 0;
    pre.setAttribute("aria-label", title);
    pre.append(element("code", JSON.stringify(value, null, 2)));
    details.append(element("summary", title), pre);
    return details;
  }

  function table(title, columns, rows) {
    const wrapper = element("div", undefined, "table-wrap");
    wrapper.tabIndex = 0;
    wrapper.setAttribute("role", "region");
    wrapper.setAttribute("aria-label", t(`${title} (scroll horizontally)`, `${title}（横スクロール可能）`));
    const grid = element("table");
    grid.append(element("caption", title));
    const head = element("thead");
    const header = element("tr");
    for (const column of columns) {
      const cell = element("th", column);
      cell.scope = "col";
      header.append(cell);
    }
    head.append(header);
    const body = element("tbody");
    for (const row of rows) {
      const tr = element("tr");
      for (const value of row) tr.append(element("td", display(value)));
      body.append(tr);
    }
    grid.append(head, body);
    wrapper.append(grid);
    if (!rows.length) wrapper.append(element("p", t("0 rows returned", "0行（取得結果）"), "small muted"));
    return wrapper;
  }

  function ledger(title, rows) {
    requireResponse(Array.isArray(rows) && rows.every(record), "Ledger rows must be objects.", "台帳の行はオブジェクトである必要があります。");
    if (!rows.length) return element("p", t(`${title}: 0 rows returned`, `${title}: 0行（取得結果）`), "small muted");
    const columns = [...new Set(rows.flatMap((row) => Object.keys(row)))];
    return table(title, columns, rows.map((row) => columns.map((column) => row[column])));
  }

  function requireQuery(result) {
    requireResponse(record(result) && typeof result.query_id === "string"
      && typeof result.semantic_revision === "string" && typeof result.truncated === "boolean"
      && Array.isArray(result.columns) && Array.isArray(result.rows)
      && result.columns.every((column) => record(column) && typeof column.name === "string")
      && result.rows.every((row) => Array.isArray(row) && row.length === result.columns.length),
    "Invalid query columns, rows, or audit information.", "クエリ結果の列・行・監査情報が不正です。");
  }

  function queryEvidence(result) {
    requireQuery(result);
    const block = element("div", undefined, "query-evidence");
    block.append(facts([
      ["query_id", result.query_id],
      ["semantic_revision", result.semantic_revision],
      ["truncated", result.truncated],
    ]));
    if (result.truncated) {
      block.append(element("p", t("Warning: the result is truncated and cannot be treated as complete.", "注意: 結果が切り詰められています。全件の結果とはみなせません。"), "notice-inline warning"));
    }
    block.append(table(t("Gateway query result", "Gateway クエリ結果"), result.columns.map((column) =>
      `${column.name}${column.type ? ` / ${display(column.type)}` : ""}`), result.rows));
    return block;
  }

  function scalar(result) {
    requireQuery(result);
    requireResponse(result.truncated === false && result.rows.length === 1
      && result.rows[0].length === 1, "Before/after revenue must be one untruncated value.", "書き込み前後の売上は、切り詰めなしの単一値が必要です。");
    return decimal(result.rows[0][0]);
  }

  function metric(label, value, className = "") {
    const block = element("div", undefined, `metric ${className}`);
    block.append(element("span", label, "metric-label"), element("strong", decimal(value), "metric-value"));
    return block;
  }

  function resultHeading(eyebrow, title, status) {
    const heading = element("div", undefined, "section-heading result-heading");
    const copy = element("div");
    copy.append(element("p", eyebrow, "eyebrow"), element("h3", title));
    heading.append(copy);
    if (status) heading.append(status);
    return heading;
  }

  function updateControls() {
    $("#compare-button").disabled = !state.ready || state.busy || !state.scenario;
    $("#ingest-button").disabled = !state.ready || state.busy;
    $("#guards-button").disabled = !state.ready || state.busy;
    $("#bootstrap-retry").disabled = state.busy;
    $("#mode-deterministic").disabled = !state.ready || state.busy;
    $("#mode-planner").disabled = !state.ready || state.busy || state.bootstrap?.planner.enabled !== true;
    document.querySelectorAll('input[name="scenario"]').forEach((input) => {
      input.disabled = !state.ready || state.busy;
    });
  }

  function setBusy(busy, english, japanese) {
    state.busy = busy;
    state.activity = [english, japanese];
    $("#activity").textContent = t(...state.activity);
    $("#activity").classList.toggle("running", busy);
    for (const id of ["compare", "ingest", "guards", "catalog"]) {
      $(`#${id}`).setAttribute("aria-busy", String(busy));
    }
    updateControls();
  }

  function clearError() {
    state.error = null;
    $("#error-panel").hidden = true;
    $("#error-message").textContent = "";
    $("#error-note").textContent = "";
  }

  function showError(error, englishTitle, japaneseTitle, isWrite) {
    state.error = {error, englishTitle, japaneseTitle, isWrite};
    $("#error-title").textContent = t(englishTitle, japaneseTitle);
    $("#error-message").textContent = error instanceof UiError
      ? error.localizedMessage() : error instanceof Error ? error.message : String(error);
    $("#error-note").textContent = isWrite
      ? t("The write may have committed despite a failed response. Neither success nor absence of a write is certain. There is no automatic retry. Check its state, then retry only the same fixed key.",
        "応答に失敗しても、書き込みが保存済みの可能性があります。成功・未保存のどちらとも断定できません。自動再試行はしません。状態確認後、同じ固定キーでのみ再実行してください。")
      : t("No substitute results or success fallback are shown. Check the connection and server before retrying.",
        "結果の代用や成功へのフォールバックは行いません。接続とサーバーの状態を確認して再実行してください。");
    $("#error-panel").hidden = false;
  }

  async function request(path, payload) {
    requireResponse(window.location.hostname === "127.0.0.1"
      || window.location.hostname === "localhost" || window.location.hostname === "[::1]",
    "This UI is loopback-only. Use the local URL printed by the server.",
    "この UI は loopback ホスト専用です。サーバーが案内するローカル URL を使用してください。");
    const options = {cache: "no-store", credentials: "same-origin", redirect: "error"};
    if (payload !== undefined) {
      const body = JSON.stringify(payload);
      requireResponse(new TextEncoder().encode(body).byteLength <= 16_384, "Requests are limited to 16 KiB.", "リクエストは 16 KiB 以下に制限されています。");
      Object.assign(options, {method: "POST", headers: {"Content-Type": "application/json"}, body});
    }
    let response;
    try {
      response = await fetch(path, options);
    } catch (error) {
      if (!(error instanceof TypeError)) throw error;
      throw new UiError("DEMO_NETWORK_ERROR: Cannot reach the local server.", "DEMO_NETWORK_ERROR: ローカルサーバーへ接続できません。");
    }
    let data;
    try {
      data = await response.json();
    } catch (error) {
      if (!(error instanceof SyntaxError || error instanceof TypeError)) throw error;
      throw new UiError(`HTTP ${response.status}: Could not read a JSON response.`, `HTTP ${response.status}: JSON 応答を取得できませんでした。`);
    }
    if (!response.ok) {
      const code = typeof data?.error?.code === "string" ? data.error.code : "DEMO_HTTP_ERROR";
      const message = typeof data?.error?.message === "string" ? data.error.message : "The server rejected the request.";
      throw new UiError(`HTTP ${response.status} · ${code}: ${message}`,
        `HTTP ${response.status} · ${code}: サーバーが要求を拒否しました。応答原文: ${message}`);
    }
    requireResponse(record(data) && data.schema_version === "1", "Unsupported schema_version.", "未対応の schema_version です。");
    return data;
  }

  function selectedMode() {
    return $('input[name="mode"]:checked').value;
  }

  function renderContext() {
    const scenario = state.bootstrap.scenarios.find((item) => item.id === state.scenario);
    const context = $("#scenario-context");
    context.replaceChildren(element("p", t("BUSINESS QUESTION", "業務の問い"), "eyebrow"),
      element("h3", scenarioText(scenario, "question")));
    context.append(facts([
      [t("Common pitfall", "見落としやすい点"), scenarioText(scenario, "pitfall")],
      [t("Business definition in this lab", "このラボの業務定義"), scenarioText(scenario, "business_rule")],
    ]));
    context.append(trace(t("Original scenario / API definition", "シナリオの原文 / API definition"), scenario));
  }

  function renderBootstrap(data) {
    requireResponse(Array.isArray(data.scenarios) && data.scenarios.length > 0
      && data.scenarios.every((item) => record(item) && ["id", "title", "question", "pitfall", "business_rule"]
        .every((key) => typeof item[key] === "string"))
      && new Set(data.scenarios.map((item) => item.id)).size === data.scenarios.length
      && Array.isArray(data.models) && data.models.every((model) => record(model)
        && typeof model.name === "string" && Number.isInteger(model.field_count) && Number.isInteger(model.metric_count))
      && typeof data.semantic_revision === "string" && record(data.contract)
      && record(data.planner) && typeof data.planner.enabled === "boolean"
      && (data.planner.model === null || typeof data.planner.model === "string")
      && typeof data.disclaimer === "string", "Invalid bootstrap scenario or published-model information.", "bootstrap のシナリオ・公開モデル情報が不正です。");
    state.bootstrap = data;
    if (state.scenario === null) state.scenario = data.scenarios[0].id;
    const scenarioNodes = data.scenarios.map((scenario, index) => {
      const copy = scenarioCopy[scenario.id];
      const label = element("label", undefined, "scenario-card");
      const input = element("input");
      input.type = "radio";
      input.name = "scenario";
      input.value = scenario.id;
      input.checked = scenario.id === state.scenario;
      input.disabled = true;
      input.addEventListener("change", () => {
        state.scenario = scenario.id;
        renderContext();
      });
      const content = element("span", undefined, "scenario-content");
      const top = element("span", undefined, "scenario-top");
      top.append(element("span", String(index + 1).padStart(2, "0"), "scenario-number"),
        element("span", copy?.category || scenario.id, "eyebrow"));
      const title = scenarioText(scenario, "title");
      content.append(top, element("strong", title));
      if (title !== scenario.title) content.append(element("span", scenario.title, "scenario-subtitle"));
      label.append(input, content);
      return label;
    });
    $("#scenarios").replaceChildren(...scenarioNodes);
    renderContext();
    $("#models").replaceChildren(...data.models.map((model) => {
      const card = element("article", undefined, "model-card");
      card.append(element("h3", model.name), element("p", t(`${model.field_count} fields · ${model.metric_count} metrics`, `${model.field_count} フィールド · ${model.metric_count} 指標`)));
      return card;
    }));
    if (!data.models.length) $("#models").append(element("p", t("No published models.", "公開モデルは 0件です。"), "muted"));
    $("#revision").textContent = data.semantic_revision;
    $("#planner-note").textContent = data.planner.enabled
      ? t(`Enabled on the server · model: ${display(data.planner.model)} · External API calls occur only in planner mode.`,
        `サーバーで有効化済み · model: ${display(data.planner.model)} · 選択時のみ外部 API を利用します。`)
      : t("Disabled · Explicit server-side opt-in is required. No API keys are entered or stored in this browser.",
        "無効 · 利用にはサーバー側での明示的な opt-in が必要です。ブラウザーでキーを入力・保存しません。");
    $("#contract").replaceChildren(trace(t("Public API contract / bootstrap", "公開 API 契約 / bootstrap"), {
      title: data.title, schema_version: data.schema_version, contract: data.contract, planner: data.planner,
    }));
    const disclaimer = element("details", undefined, "trace");
    disclaimer.append(element("summary", t("Original server comparison disclaimer", "比較についてのサーバー説明（原文）")), element("p", data.disclaimer));
    $("#contract").append(disclaimer);
  }

  function renderCompare(data) {
    const {baseline, semantic, expected, source, comparison} = data;
    requireResponse(record(data.scenario) && record(baseline) && record(semantic)
      && record(expected) && record(source) && record(comparison)
      && typeof baseline.correct === "boolean" && typeof semantic.correct === "boolean"
      && typeof baseline.sql === "string" && typeof baseline.role === "string"
      && typeof expected.derivation === "string" && typeof source.fingerprint === "string"
      && typeof comparison.same_role === "boolean" && typeof comparison.stable_source === "boolean"
      && record(semantic.validation) && record(semantic.explanation) && record(semantic.lsq)
      && ["deterministic", "planner"].includes(data.mode)
      && (data.planner === null || record(data.planner)), "Required comparison evidence is missing.", "比較結果の必須情報が欠けています。");
    requireQuery(semantic.result);
    const output = element("div");
    output.append(resultHeading(t("ACTUAL DATABASE RESULTS", "実データベースの結果"), scenarioText(data.scenario, "title")));
    output.append(element("p", `${data.mode === "planner" ? t("OpenAI candidate selection", "OpenAI 候補選択") : t("Authored fixed plans", "事前定義の固定計画")} · ${scenarioText(data.scenario, "question")}`, "small muted"));
    const checks = element("div", undefined, "condition-bar");
    checks.append(badge(comparison.same_role, t("Same reader role", "同一の読み取りロール"), t("Role mismatch", "ロールが不一致")),
      badge(comparison.stable_source, t("Source stable during execution", "実行中のソースは安定"), t("Source changed during execution", "実行中にソースが変化")));
    output.append(checks);
    if (!comparison.same_role || !comparison.stable_source) {
      output.append(element("p", t("Comparison conditions do not match. These are returned records, not a fair same-condition comparison. Check concurrent writers and connection settings.",
        "比較条件が成立していません。以下は返却された記録であり、公平な同一条件の比較とはみなせません。外部の更新や接続設定を確認してください。"),
        "notice warning"));
    }
    const pair = element("div", undefined, "comparison-grid");
    const baselinePanel = element("article", undefined, "panel plan-panel baseline-panel");
    baselinePanel.append(resultHeading(t("SCHEMA-ONLY / DIRECT SQL", "スキーマのみ / 直接 SQL"), t("SQL baseline", "SQL 比較対象"),
      badge(baseline.correct, t("Matches expected", "期待値と一致"), t("Does not match", "期待値と不一致"))));
    baselinePanel.append(element("p", t("Direct PostgreSQL / schema-only plan", "直接 PostgreSQL / スキーマだけの計画"), "small muted"), metric(t("Returned value / actual", "返却値 / actual"), baseline.value));
    baselinePanel.append(facts([[t("Selected candidate", "選択した候補"), baseline.choice], [t("Reader role", "読み取りロール"), baseline.role]]));
    baselinePanel.append(element("p", data.mode === "deterministic"
      ? t("Authored schema-only mistake; not a live model output.", "事前定義したスキーマだけの計画ミスであり、実モデルの出力ではありません。")
      : baseline.explanation, "plan-explanation"));
    const sql = element("details", undefined, "trace sql-trace");
    sql.open = true;
    const sqlCode = element("pre");
    sqlCode.tabIndex = 0;
    sqlCode.setAttribute("aria-label", t("Fixed SQL for this demo only", "デモ専用の固定 SQL"));
    sqlCode.append(element("code", baseline.sql));
    sql.append(element("summary", t("Reviewed, demo-only fixed SQL", "信頼済み・デモ専用の固定 SQL")),
      element("p", t("Executed through the isolated read-only comparison path. This is not gateway SQL output; the browser cannot submit SQL.",
        "比較用の隔離された読み取り経路で実行。Gateway の SQL 出力ではなく、ブラウザーから SQL を送ることもできません。"), "small muted"),
      sqlCode);
    baselinePanel.append(sql);

    const semanticPanel = element("article", undefined, "panel plan-panel semantic-panel");
    semanticPanel.append(resultHeading(t("GOVERNED / SEMANTIC MCP", "統制された実行 / SEMANTIC MCP"), t("Query published business meaning", "公開された意味で問い合わせる"),
      badge(semantic.correct, t("Matches expected", "期待値と一致"), t("Does not match", "期待値と不一致"))));
    semanticPanel.append(element("p", t("postgresem / published semantic plan", "postgresem / 公開セマンティック計画"), "small muted"), metric(t("Returned value / actual", "返却値 / actual"), semantic.value));
    semanticPanel.append(facts([[t("Selected candidate", "選択した候補"), semantic.choice]]),
      badge(semantic.validation.valid, t("LSQ validated", "LSQ 検証を通過"), t("LSQ rejected", "LSQ 検証で拒否")),
      element("p", t("Select a published metric using LSQ, then validate, explain, and execute it through MCP. A well-typed request can still choose the wrong metric; compare its answer with the business expectation.",
        "公開指標を LSQ で指定し、検証・説明・クエリ実行を MCP 経由で行います。型が正しい要求でも、問いに合う指標を選べたかは期待値との比較が必要です。"), "plan-explanation"));
    semanticPanel.append(trace(t("Submitted LSQ", "送信された LSQ"), semantic.lsq), queryEvidence(semantic.result));
    pair.append(baselinePanel, semanticPanel);
    output.append(pair);

    const answer = element("section", undefined, "expected-panel");
    answer.append(metric(t("Independent business-rule expectation", "業務定義から独立に計算した期待値"), expected.value));
    const derivation = element("div", undefined, "derivation");
    const calculation = state.language === "ja"
      ? expected.derivation.replace("Sum monthly amounts of active subscriptions only.", "有効な契約の月額だけを合計します。")
        .replace("(each qualifying order once)", "（該当する注文を一度ずつ計上）")
      : expected.derivation;
    derivation.append(element("h4", t("Calculation / derivation", "計算の根拠 / derivation")), element("p", calculation),
      element("p", scenarioText(data.scenario, "business_rule"), "small"));
    answer.append(derivation);
    output.append(answer);
    output.append(element("p", t("Decimal strings are displayed exactly as returned by the API. If the selected metric is a count, its unit is not a monetary amount.",
      "値は API の十進数文字列をそのまま表示しています。選ばれた指標が件数などの場合、返却値は金額と同じ単位ではありません。"), "small muted"));
    const verdict = element("div", undefined, "notice verdict");
    if (!comparison.same_role || !comparison.stable_source) {
      verdict.append(element("strong", t("The conditions do not match, so no superiority can be inferred.", "同一条件を確認できないため、優劣は判断できません。")));
    } else if (baseline.correct && semantic.correct) {
      verdict.append(element("strong", t("Both answers match. Correct SQL returns the correct answer too.", "両方とも期待値に一致しました。正しい SQL も、正しい答えを返します。")));
    } else if (!baseline.correct && !semantic.correct) {
      verdict.append(element("strong", t("Neither answer matches. A semantic model alone does not guarantee the right metric selection.", "両方とも期待値に不一致です。意味モデルの利用だけで、指標選択の正しさは保証されません。")));
    } else {
      verdict.append(element("strong", baseline.correct
        ? t("The SQL baseline matches; the semantic answer does not. Review the selected metric and business definition.", "SQL baseline が一致し、semantic 側は不一致です。選んだ指標と業務定義を確認してください。")
        : t("The semantic answer matches; the SQL baseline does not. Review filters and aggregation grain.", "semantic 側が一致し、SQL baseline は不一致です。集計条件と粒度を確認してください。")));
    }
    verdict.append(element("p", t("Compare each actual result with the independently calculated business answer.", "各実行結果を、独立して計算した業務上の正解と比較してください。")));
    output.append(verdict);
    if (data.planner !== null) {
      const planner = element("section", undefined, "panel planner-result");
      planner.append(element("h4", t("Planner reasoning / original model output", "Planner の選択理由 / モデル出力原文")),
        facts([[t("Model", "モデル"), data.planner.model], ["SQL baseline", data.planner.baseline_reason],
          ["Semantic MCP", data.planner.semantic_reason]]));
      output.append(planner);
    }
    const evidence = element("details", undefined, "panel evidence");
    evidence.append(element("summary", t("Execution traces / validate · explain · query", "実行トレース / validate · explain · query")),
      trace(t("Validation JSON", "検証 JSON"), semantic.validation), trace(t("Explanation JSON", "実行説明 JSON"), semantic.explanation),
      trace(t("Query result JSON", "クエリ結果 JSON"), semantic.result));
    if (semantic.catalog) evidence.append(trace(t("Published catalog JSON", "公開カタログ JSON"), semantic.catalog));
    evidence.append(trace(t("Complete comparison response", "比較応答全体 / complete response"), data));
    output.append(evidence);
    const sources = element("details", undefined, "panel evidence");
    sources.append(element("summary", t("Source ledger and contributing rows / PostgreSQL snapshot", "ソース台帳と寄与した行 / PostgreSQL snapshot")),
      element("p", t("Real data fetched during this comparison. Rerun the comparison for current values after a write.",
        "同じ比較実行で取得した実データです。書き込み後の最新値を見るには比較を再実行してください。"), "small muted"),
      facts([[t("Source fingerprint", "ソースのフィンガープリント"), source.fingerprint]]),
      ledger(t("Contributing rows", "期待値に寄与した行 / contributing_rows"), expected.contributing_rows),
      ledger(t("Orders", "注文 / orders"), source.orders), ledger(t("Order items", "注文明細 / items"), source.items),
      ledger(t("Subscriptions", "契約 / subscriptions"), source.subscriptions));
    output.append(sources);
    return output;
  }

  function auditIds(value, prefix = "") {
    const entries = [];
    if (!record(value) && !Array.isArray(value)) return entries;
    for (const [key, child] of Object.entries(value)) {
      const path = prefix ? `${prefix}.${key}` : key;
      if (key.endsWith("_id") && child !== null && typeof child !== "object") entries.push([path, child]);
      else if (record(child) || Array.isArray(child)) entries.push(...auditIds(child, path));
    }
    return entries;
  }

  function renderIngest(data) {
    requireResponse(record(data.first) && record(data.replay) && record(data.reconciliation)
      && record(data.reconciliation.state) && typeof data.reconciliation.state.status === "string"
      && record(data.validation) && record(data.conflicting_retry)
      && typeof data.conflicting_retry.rejected === "boolean" && typeof data.conflicting_retry.code === "string"
      && typeof data.consistent === "boolean",
    "Invalid mutation or reconciliation result format.", "書き込み・照合の結果形式が不正です。");
    const before = scalar(data.before);
    const after = scalar(data.after);
    const output = element("div");
    output.append(resultHeading(t("WRITE / REPLAY / RECONCILE", "書き込み / 再実行 / 照合"), t("Persistence and replay records", "保存と再実行の記録"),
      badge(data.consistent, t("Consistent", "整合性を確認"), t("Inconsistent", "整合性に不一致"))));
    if (!data.consistent) output.append(element("p",
      t("The delta, replay, or reconciliation does not match expectations. A write may have committed. Inspect the audit records.",
        "差分・再実行・照合のいずれかが期待と一致していません。保存済みの可能性があります。監査記録を確認してください。"), "notice warning"));
    const values = element("div", undefined, "write-metrics panel");
    values.append(metric(t("Revenue / before", "売上 / before"), before), metric(t("Revenue / after", "売上 / after"), after),
      metric(t("Actual delta", "実際の差分 / actual delta"), data.actual_delta), metric(t("Expected delta", "期待する差分 / expected delta"), data.expected_delta));
    output.append(values);
    const replayInfo = element("div", undefined, "condition-bar");
    replayInfo.append(badge(data.validation.valid, t("LSM validated", "LSM 検証を通過"), t("LSM rejected", "LSM 検証で拒否")),
      badge(data.replay.replayed, t("Same-key replay confirmed", "同じキーの再実行を確認"), t("Replay not confirmed", "再実行を確認できず")));
    output.append(replayInfo);
    output.append(element("p", data.first.replayed === true
      ? t("This key has already been recorded. The first call is also a replay; no new order is added.", "このキーは投入済みです。今回の最初の呼び出しも replay であり、新しい注文は追加しません。")
      : data.first.replayed === false
        ? t("The first call is a new write. Inspect the subsequent same-key call below.", "最初の呼び出しは新規書き込みです。続く同一キーの呼び出し結果を下で確認できます。")
        : t("The first call's replay status is unknown. Inspect its trace.", "最初の呼び出しの replay 状態を確認できません。トレースを確認してください。"), "small"));
    const actions = element("div", undefined, "comparison-grid");
    for (const [label, mutation] of [[t("First call", "最初の呼び出し / first"), data.first], [t("Same-key replay", "同じキーで再実行 / replay"), data.replay]]) {
      const panel = element("article", undefined, "panel");
      panel.append(element("h4", label), facts([
        ["mutation_id", mutation.mutation_id], ["affected_rows", mutation.affected_rows],
        ["replayed", mutation.replayed], ["semantic_revision", mutation.semantic_revision],
      ]));
      actions.append(panel);
    }
    output.append(actions);
    const reconciliation = element("div", undefined, "panel");
    reconciliation.append(element("h4", t("Persisted-state reconciliation", "保存状態の照合 / reconciliation")),
      badge(data.reconciliation.state.status === "committed", t("Committed", "committed を確認"), t("Not committed", "committed ではない")),
      facts([["state.status", data.reconciliation.state.status],
        ["state.mutation_id", data.reconciliation.state.mutation_id]]));
    output.append(reconciliation);
    const conflict = element("div", undefined, "notice");
    conflict.append(element("strong", t("Same key, changed content / conflicting retry", "同じキーで内容だけを変更した再試行 / conflicting retry")),
      facts([["rejected", data.conflicting_retry.rejected],
        ["code", data.conflicting_retry.code]]),
      element("p", t("This rejection code is the actual response. See the traces for reconciliation and conflict details.", "拒否コードは実際の応答です。照合・競合の詳細はトレースを参照してください。"), "small"));
    output.append(conflict);
    const audits = element("div", undefined, "panel");
    audits.append(element("h4", t("Audit IDs / before · mutation · replay · reconciliation · after", "監査 ID / before · mutation · replay · reconciliation · after")),
      facts(auditIds(data), "audit-facts"));
    output.append(audits);
    const evidence = element("details", undefined, "panel evidence");
    evidence.append(element("summary", t("Complete write traces / validation · reconciliation · conflict", "書き込みの全トレース / validation · reconciliation · conflict")),
      trace(t("LSM validation", "LSM の検証"), data.validation), trace(t("First mutation", "最初の書き込み"), data.first),
      trace(t("Idempotent replay", "冪等な再実行"), data.replay), trace(t("Reconciliation", "結果照合"), data.reconciliation),
      trace(t("Conflicting retry", "競合する再試行"), data.conflicting_retry), trace(t("Complete response", "全応答 / complete response"), data));
    output.append(evidence);
    const queries = element("details", undefined, "panel evidence");
    queries.append(element("summary", t("Before and after queries", "書き込み前後のクエリ / before · after")),
      element("h4", t("Before", "書き込み前")), queryEvidence(data.before), element("h4", t("After", "書き込み後")), queryEvidence(data.after));
    output.append(queries);
    return output;
  }

  function renderGuards(data) {
    requireResponse(typeof data.passed === "boolean" && Array.isArray(data.rejections)
      && data.rejections.every((item) => record(item) && typeof item.name === "string"
        && typeof item.code === "string" && typeof item.rejected === "boolean")
      && Array.isArray(data.tenants) && data.tenants.every((item) => record(item)
        && typeof item.name === "string" && typeof item.role === "string"
        && typeof item.query_id === "string" && typeof item.rls_enforced === "boolean"),
    "Invalid rejection or tenant result format.", "拒否結果・テナント結果の形式が不正です。");
    const output = element("div");
    output.append(resultHeading(t("TYPED REJECTION / POSTGRESQL RLS", "型付きの拒否 / POSTGRESQL RLS"), t("Rejection and fixed-tenant results", "拒否と固定テナントの実行結果"),
      badge(data.passed, t("Checks passed", "チェックを通過"), t("Checks did not agree", "チェックに不一致"))));
    const rejections = element("div", undefined, "rejection-grid");
    const names = {"Hidden metric": "非公開の指標", "Unknown metric": "存在しない指標", "Raw SQL": "raw SQL の持ち込み"};
    for (const rejection of data.rejections) {
      const card = element("article", undefined, "panel rejection-card");
      card.append(badge(rejection.rejected, t("Request rejected", "要求を拒否"), t("Not rejected", "拒否されていない")),
        element("h4", state.language === "ja" ? names[rejection.name] || rejection.name : rejection.name), element("code", rejection.code));
      rejections.append(card);
    }
    if (!data.rejections.length) rejections.append(element("p", t("No rejection results. Review what was checked.", "拒否結果は 0件です。検証内容を確認してください。"), "notice warning"));
    output.append(rejections);
    output.append(element("p",
      t("Direct SQL and semantic values use the same fixed tenant role. RLS is a PostgreSQL feature and applies to both paths.",
        "同じ Tenant の固定ロールで直接 SQL と semantic の値を並べます。RLS は PostgreSQL の機能であり、両経路に適用されます。"),
      "small muted"));
    const tenants = element("div", undefined, "comparison-grid");
    for (const tenant of data.tenants) {
      const card = element("article", undefined, "panel");
      card.append(resultHeading(t("FIXED DATABASE IDENTITY", "固定データベース主体"), tenant.name,
        badge(tenant.rls_enforced, t("RLS confirmed", "RLS を確認"), t("RLS mismatch", "RLS の検証に不一致"))));
      const values = element("div", undefined, "tenant-values");
      values.append(metric(t("Direct SQL", "直接 SQL"), tenant.direct_value), metric("Semantic MCP", tenant.semantic_value));
      card.append(values, facts([[t("PostgreSQL role", "PostgreSQL ロール"), tenant.role], ["query_id", tenant.query_id]]));
      tenants.append(card);
    }
    if (!data.tenants.length) tenants.append(element("p", t("No tenant results. Isolation has not been confirmed.", "テナント結果は 0件です。分離は確認できません。"), "notice warning"));
    output.append(tenants, trace(t("Complete typed-rejection and RLS response", "型付き拒否と RLS の全応答 / complete response"), data));
    return output;
  }

  const renderers = {compare: renderCompare, ingest: renderIngest, guards: renderGuards};

  function paintConnection() {
    const labels = {
      connecting: ["Connecting", "接続を確認中"],
      ready: ["Connected / LOCAL", "接続確認済み / LOCAL"],
      failed: ["Connection failed", "接続できません"],
    };
    $("#connection-status").textContent = t(...labels[state.connection]);
    $("#connection-status").className = `badge ${state.connection === "ready" ? "success" : state.connection === "failed" ? "failure" : "neutral"}`;
  }

  function paintWorkflowStatus(kind) {
    const [english, japanese] = workflowLabels[kind];
    const phase = state.workflows[kind];
    if (phase === "loading") {
      $(`#${kind}-empty`).textContent = t(`${english} is running. Waiting for the actual response...`, `${japanese}を実行中です。実際の応答を待っています…`);
    } else if (phase === "error") {
      $(`#${kind}-empty`).textContent = t(`Could not fetch or display ${english.toLowerCase()} results. See the error above.`, `${japanese}の結果を取得・表示できませんでした。画面上部のエラーを確認してください。`);
    }
  }

  function paintResult(kind) {
    $(`#${kind}-result`).replaceChildren(renderers[kind](state.results[kind]));
    if (kind === "compare" && state.comparisonStale) {
      $("#compare-result").prepend(element("p", t(
        "A write was requested after this comparison. These are pre-write results. Rerun the comparison for current values.",
        "この比較の後に書き込みを要求しました。これは書き込み前の結果です。最新値は比較を再実行して確認してください。",
      ), "notice warning stale-notice"));
    }
  }

  function setLanguage(language) {
    if (!["en", "ja"].includes(language)) {
      $("#language-select").value = state.language;
      showError(new UiError("Unsupported language.", "未対応の言語です。"), "Language switch failed", "言語の切り替えに失敗しました", false);
      return;
    }
    const openDetails = [...document.querySelectorAll("details")].map((details) => details.open);
    state.language = language;
    document.documentElement.lang = language;
    $("#language-select").value = language;
    for (const {node, text, label, content} of staticText) {
      if (node.hasAttribute("data-ja")) node.textContent = t(text, node.dataset.ja);
      if (node.hasAttribute("data-ja-label")) node.setAttribute("aria-label", t(label, node.dataset.jaLabel));
      if (node.hasAttribute("data-ja-content")) node.setAttribute("content", t(content, node.dataset.jaContent));
    }
    if (state.bootstrap !== null) renderBootstrap(state.bootstrap);
    for (const kind of Object.keys(renderers)) {
      if (state.results[kind]) paintResult(kind);
      paintWorkflowStatus(kind);
    }
    document.querySelectorAll("details").forEach((details, index) => {
      if (index < openDetails.length) details.open = openDetails[index];
    });
    paintConnection();
    if (state.activity) $("#activity").textContent = t(...state.activity);
    if (state.error) {
      const {error, englishTitle, japaneseTitle, isWrite} = state.error;
      showError(error, englishTitle, japaneseTitle, isWrite);
    }
    updateControls();
  }

  async function workflow(kind, payload) {
    if (state.busy || !state.ready) return;
    clearError();
    const [english, japanese] = workflowLabels[kind];
    const result = $(`#${kind}-result`);
    const empty = $(`#${kind}-empty`);
    delete state.results[kind];
    state.workflows[kind] = "loading";
    result.hidden = true;
    result.replaceChildren();
    empty.hidden = false;
    paintWorkflowStatus(kind);
    setBusy(true, `${english} is running. Other operations are disabled until it finishes.`, `${japanese}を実行中 · 他の操作は完了まで無効です。`);
    let completed = false;
    try {
      const data = await request(`/api/${kind}`, payload);
      if (kind === "compare") requireResponse(data.scenario?.id === payload.scenario && data.mode === payload.mode,
        "Comparison response scenario or mode does not match the request.", "要求と比較応答のシナリオ・モードが一致しません。");
      const content = renderers[kind](data);
      state.results[kind] = data;
      state.workflows[kind] = "success";
      if (kind === "compare") state.comparisonStale = false;
      result.replaceChildren(content);
      result.hidden = false;
      empty.hidden = true;
      completed = true;
    } catch (error) {
      state.workflows[kind] = "error";
      paintWorkflowStatus(kind);
      showError(error, `${english} failed`, `${japanese}に失敗しました`, kind === "ingest");
    } finally {
      setBusy(false, completed
        ? `${english} response received. Read the result badges for outcomes and comparison conditions.`
        : `${english} failed. No substitute data is shown.`,
      completed
        ? `${japanese}の応答を受信しました。成否と比較条件は各結果のバッジを確認してください。`
        : `${japanese}は失敗しました。代替データは表示していません。`);
      if (completed) result.focus({preventScroll: true});
      else $("#error-panel").focus();
    }
  }

  async function bootstrap() {
    if (state.busy) return;
    clearError();
    state.ready = false;
    state.connection = "connecting";
    paintConnection();
    setBusy(true, "Fetching published models and scenarios. Operations are not available yet.", "公開モデルとシナリオを取得中 · 操作はまだ実行できません。");
    try {
      const data = await request("/api/bootstrap");
      renderBootstrap(data);
      state.ready = true;
      $("#bootstrap-retry").hidden = true;
      state.connection = "ready";
    } catch (error) {
      state.connection = "failed";
      $("#bootstrap-retry").hidden = false;
      showError(error, "Could not initialize the lab", "ラボを初期化できませんでした", false);
    } finally {
      paintConnection();
      setBusy(false, state.ready
        ? "Ready. Choose a scenario to compare. No result values have been fetched yet."
        : "Initialization failed. Inspect the error and retry the connection.",
      state.ready
        ? "準備できました。シナリオを選んで比較してください。数値はまだ取得していません。"
        : "初期化に失敗しました。実際のエラーを確認し、接続を再確認してください。");
      if (!state.ready) $("#error-panel").focus();
    }
  }

  $("#compare-button").addEventListener("click", () => {
    workflow("compare", {scenario: state.scenario, mode: selectedMode()});
  });
  $("#ingest-button").addEventListener("click", () => {
    if (state.busy || !state.ready) return;
    const confirmed = window.confirm(t(
      "This will persist a real 45.00 paid order in the local fictional PostgreSQL database.\n\n"
      + "Changes persist and are not automatically undone. The same fixed idempotency key is replayed and reconciled, and a changed-content retry is rejected.\n"
      + "If the order was already recorded, repeating this action does not add another order.\n\nProceed?",
      "ローカルの架空データ用 PostgreSQL に、45.00 の確定注文を実際に保存します。\n\n"
      + "変更は永続化し、自動で元に戻しません。同じ固定の冪等キーで再実行・照合し、内容を変えた再試行の拒否も検証します。\n"
      + "投入済みの場合、繰り返しても注文は増えません。\n\n実行しますか？"
    ));
    if (!confirmed) {
      setBusy(false, "Write cancelled. No request was sent.", "書き込みをキャンセルしました。リクエストは送信していません。");
      return;
    }
    if (!$("#compare-result").hidden) {
      state.comparisonStale = true;
      paintResult("compare");
    }
    workflow("ingest", {action: "record-paid-order"});
  });
  $("#guards-button").addEventListener("click", () => {
    workflow("guards", {});
  });
  $("#bootstrap-retry").addEventListener("click", bootstrap);
  $("#language-select").addEventListener("change", (event) => setLanguage(event.target.value));
  setLanguage("en");
  bootstrap();
})();
