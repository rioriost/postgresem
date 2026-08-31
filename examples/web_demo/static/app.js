"use strict";

const examplesElement = document.querySelector("#examples");
const modelsElement = document.querySelector("#models");
const statusElement = document.querySelector("#connection-status");
const revisionElement = document.querySelector("#revision");
const runStateElement = document.querySelector("#run-state");
const emptyElement = document.querySelector("#result-empty");
const contentElement = document.querySelector("#result-content");
const factsElement = document.querySelector("#facts");
const headElement = document.querySelector("#result-head");
const bodyElement = document.querySelector("#result-body");
const evidenceElement = document.querySelector("#evidence");

function shortHash(value) {
  if (typeof value !== "string") return "unavailable";
  return value.length > 24 ? `${value.slice(0, 19)}…` : value;
}

function text(tag, value, className) {
  const element = document.createElement(tag);
  element.textContent = value;
  if (className) element.className = className;
  return element;
}

function renderModels(models) {
  modelsElement.replaceChildren();
  for (const model of models) {
    const item = text("article", "", "model-chip");
    item.append(
      text("strong", model.name),
      text(
        "span",
        `${model.field_count} fields · ${model.metric_count} metrics`
      )
    );
    modelsElement.append(item);
  }
}

function renderExamples(examples) {
  examplesElement.replaceChildren();
  for (const example of examples) {
    const card = text("article", "", "query-card");
    card.append(
      text("span", example.model, "model-label"),
      text("h3", example.title),
      text("p", example.description)
    );
    const button = text("button", "Run governed query");
    button.type = "button";
    button.addEventListener("click", () => runExample(example.id, button));
    card.append(button);
    examplesElement.append(card);
  }
}

function renderEvidence(validation, explanation, result) {
  const fields = [
    ["Validation", validation.valid ? "Accepted" : "Rejected"],
    [
      "Semantic lineage",
      (explanation.semantic_models || []).join(", ") || "Unavailable",
    ],
    ["Audit query ID", result.query_id || "Unavailable"],
    ["Truncated", String(Boolean(result.truncated))],
  ];
  evidenceElement.replaceChildren();
  for (const [label, value] of fields) {
    const row = document.createElement("div");
    row.append(text("dt", label), text("dd", value));
    evidenceElement.append(row);
  }
}

function renderResult(response) {
  const {validation, explanation, result} = response;
  if (!validation.valid || !result) {
    throw new Error(
      validation.error?.message || "The semantic query was rejected."
    );
  }

  factsElement.replaceChildren(
    text("span", `revision ${shortHash(result.semantic_revision)}`),
    text("span", `${result.rows.length} row(s)`),
    text("span", `${result.columns.length} column(s)`)
  );
  headElement.replaceChildren();
  bodyElement.replaceChildren();

  const headerRow = document.createElement("tr");
  for (const column of result.columns) {
    const heading = text("th", column.name);
    heading.scope = "col";
    heading.title = column.type;
    headerRow.append(heading);
  }
  headElement.append(headerRow);

  for (const row of result.rows) {
    const tableRow = document.createElement("tr");
    for (const value of row) {
      tableRow.append(text("td", value === null ? "null" : String(value)));
    }
    bodyElement.append(tableRow);
  }

  renderEvidence(validation, explanation, result);
  emptyElement.hidden = true;
  contentElement.hidden = false;
}

async function runExample(example, button) {
  const buttons = document.querySelectorAll("button");
  buttons.forEach((item) => { item.disabled = true; });
  runStateElement.textContent = "Validating and executing";
  runStateElement.className = "badge running";
  try {
    const response = await fetch("/api/run", {
      method: "POST",
      headers: {"Content-Type": "application/json"},
      body: JSON.stringify({example}),
    });
    const payload = await response.json();
    if (!response.ok) {
      throw new Error(payload.error?.message || "Request failed.");
    }
    renderResult(payload);
    runStateElement.textContent = "Audited success";
    runStateElement.className = "badge success";
  } catch (error) {
    emptyElement.hidden = false;
    contentElement.hidden = true;
    emptyElement.textContent = error.message;
    runStateElement.textContent = "Failed safely";
    runStateElement.className = "badge failure";
  } finally {
    buttons.forEach((item) => { item.disabled = false; });
    button.focus();
  }
}

async function bootstrap() {
  try {
    const response = await fetch("/api/bootstrap");
    const payload = await response.json();
    if (!response.ok) throw new Error("Gateway unavailable.");
    statusElement.textContent = "Gateway ready";
    revisionElement.textContent = `Revision ${shortHash(payload.semantic_revision)}`;
    renderExamples(payload.examples);
    renderModels(payload.models);
  } catch (error) {
    statusElement.textContent = "Gateway unavailable";
    revisionElement.textContent = error.message;
    examplesElement.append(text("p", "Could not load demo queries."));
  }
}

bootstrap();

