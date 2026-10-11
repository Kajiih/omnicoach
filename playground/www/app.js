import { loadOmnicoach } from "./omnicoach.js";

const PRESETS = {
  "python-test": {
    path: "tests/test_order.py",
    code: [
      "import time",
      "",
      "",
      "def test_order_processing():",
      "    time.sleep(0.5)",
      "    res = [10, 20]",
      "    assert res[0] == 10 and res[1] == 20",
      "",
    ].join("\n"),
    config: "",
  },
  "python-source": {
    path: "src/service.py",
    code: [
      "from typing import Optional",
      "",
      "DEFAULT_ITEMS = [\"a\", \"b\"]",
      "",
      "",
      "def fetch_user(user_list: list[str], timeout_ms: Optional[int] = None) -> list[str]:",
      "    try:",
      "        return [item for item in user_list]",
      "    except Exception:",
      "        return []",
      "",
    ].join("\n"),
    config: "",
  },
  "rust-source": {
    path: "src/lib.rs",
    code: [
      "pub fn compute_total(price_list: &Vec<u64>, timeout_ms: u64) -> u64 {",
      "    let cnt = price_list.len() as u64;",
      "    cnt + timeout_ms",
      "}",
      "",
    ].join("\n"),
    config: "",
  },
  "rust-test": {
    path: "tests/integration_test.rs",
    code: [
      "use std::thread;",
      "use std::time::Duration;",
      "",
      "#[test]",
      "fn test_worker_completes() {",
      "    thread::sleep(Duration::from_millis(50));",
      "    assert!(1 == 1 && 2 == 2);",
      "}",
      "",
    ].join("\n"),
    config: "",
  },
};

const pathInput = document.getElementById("path-input");
const presetSelect = document.getElementById("preset-select");
const codeEditor = document.getElementById("code-editor");
const configEditor = document.getElementById("config-editor");
const languageBadge = document.getElementById("language-badge");
const statusBadge = document.getElementById("status-badge");
const outputBody = document.getElementById("output-body");
const explainToolbar = document.getElementById("explain-toolbar");
const ruleSelect = document.getElementById("rule-select");
const shareButton = document.getElementById("share-button");
const tabButtons = Array.from(document.querySelectorAll(".tab-button"));

let client = null;
let activeTab = "cards";
let latestResult = null;
let selectedRule = "sleep-in-tests";

function updateLanguageBadge(result) {
  if (!result || !result.ok) {
    languageBadge.textContent = "Config error";
    return;
  }
  const scopeSuffix = result.is_test_path ? "test path" : "source path";
  if (result.language === "python") {
    languageBadge.textContent = `Python (${scopeSuffix})`;
  } else if (result.language === "rust") {
    languageBadge.textContent = `Rust (${scopeSuffix})`;
  } else {
    languageBadge.textContent = "Unsupported extension (use .py or .rs)";
  }
}

function setStatusBadge(text, kind) {
  statusBadge.textContent = text;
  statusBadge.className = `badge badge-${kind}`;
}

function createDetailRow(label, value) {
  const row = document.createElement("div");
  row.className = "diag-detail";
  const strong = document.createElement("span");
  strong.className = "diag-detail-label";
  strong.textContent = `${label}: `;
  const body = document.createElement("span");
  body.textContent = value;
  row.appendChild(strong);
  row.appendChild(body);
  return row;
}

function renderCards(result) {
  outputBody.replaceChildren();
  if (!result.ok) {
    const errorBox = document.createElement("div");
    errorBox.className = "error-box";
    errorBox.textContent = result.error;
    outputBody.appendChild(errorBox);
    return;
  }

  if (result.diagnostics.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "All checks passed! (0 diagnostics)";
    outputBody.appendChild(empty);
    return;
  }

  const list = document.createElement("div");
  list.className = "diag-list";
  for (const diag of result.diagnostics) {
    const card = document.createElement("div");
    card.className = "diag-card";

    const header = document.createElement("div");
    header.className = "diag-header";

    const loc = document.createElement("span");
    loc.className = "diag-location";
    const file = diag.location.file_path || pathInput.value;
    const line = diag.location.line || 1;
    const col = diag.location.column || 1;
    loc.textContent = `${file}:${line}:${col}`;

    const ruleButton = document.createElement("button");
    ruleButton.type = "button";
    ruleButton.className = "rule-pill";
    ruleButton.title = "Click to view --explain documentation for this rule";
    ruleButton.textContent = `[${diag.rule_name}]`;
    ruleButton.addEventListener("click", () => {
      selectedRule = diag.rule_name;
      ruleSelect.value = selectedRule;
      switchTab("explain");
    });

    header.appendChild(loc);
    header.appendChild(ruleButton);

    const summary = document.createElement("div");
    summary.className = "diag-summary";
    summary.textContent = diag.message.summary;

    card.appendChild(header);
    card.appendChild(summary);
    card.appendChild(createDetailRow("Rationale", diag.message.rationale));
    card.appendChild(createDetailRow("Suggestion", diag.message.suggestion));
    if (diag.message.explanation_hint) {
      card.appendChild(createDetailRow("Or", diag.message.explanation_hint));
    }

    list.appendChild(card);
  }
  outputBody.appendChild(list);
}

function renderPreformatted(text) {
  outputBody.replaceChildren();
  const pre = document.createElement("pre");
  pre.className = "pre-output";
  pre.textContent = text;
  outputBody.appendChild(pre);
}

function renderApiInstructions() {
  const origin = window.location.origin + window.location.pathname.replace(/\/[^/]*$/, "");
  const payload = {
    action: "lint",
    path: pathInput.value,
    code: codeEditor.value,
    config: configEditor.value,
  };
  const snippet = [
    "// Programmatic request/response from JavaScript, Node.js, Deno, Bun, or Cloudflare Workers:",
    `import { loadOmnicoach } from "${origin}/omnicoach.js";`,
    "",
    `const omni = await loadOmnicoach("${origin}/omnicoach_playground.wasm");`,
    `const response = omni.request(${JSON.stringify(payload, null, 2)});`,
    "// response => { ok: true, diagnostics: [...], plain: \"...\" }",
  ].join("\n");
  renderPreformatted(snippet);
}

function renderActiveView() {
  explainToolbar.classList.toggle("hidden", activeTab !== "explain");
  if (!client || !latestResult) {
    return;
  }

  if (activeTab === "cards") {
    renderCards(latestResult);
  } else if (activeTab === "plain") {
    if (!latestResult.ok) {
      renderCards(latestResult);
    } else {
      renderPreformatted(latestResult.plain || "All checks passed!");
    }
  } else if (activeTab === "json") {
    renderPreformatted(JSON.stringify(latestResult, null, 2));
  } else if (activeTab === "explain") {
    const explainResult = client.explain(selectedRule, configEditor.value);
    if (explainResult.ok) {
      renderPreformatted(explainResult.markdown);
    } else {
      renderCards(explainResult);
    }
  } else if (activeTab === "api") {
    renderApiInstructions();
  }
}

function switchTab(tabName) {
  activeTab = tabName;
  for (const button of tabButtons) {
    button.classList.toggle("active", button.dataset.tab === tabName);
  }
  renderActiveView();
}

function runLint() {
  if (!client) {
    return;
  }
  try {
    latestResult = client.lint({
      path: pathInput.value.trim(),
      code: codeEditor.value,
      config: configEditor.value,
    });
  } catch (runtimeError) {
    latestResult = {
      ok: false,
      error: `WASM runtime error: ${String(runtimeError)}`,
    };
  }

  updateLanguageBadge(latestResult);
  if (!latestResult.ok) {
    setStatusBadge("Error", "err");
  } else if (latestResult.diagnostics.length === 0) {
    setStatusBadge("0 violations", "ok");
  } else {
    const count = latestResult.diagnostics.length;
    setStatusBadge(`${count} violation${count === 1 ? "" : "s"}`, "warn");
  }
  renderActiveView();
}

function populateRuleCatalog() {
  const catalogResponse = client.catalog();
  if (!catalogResponse.ok) {
    return;
  }
  ruleSelect.replaceChildren();
  for (const entry of catalogResponse.rules) {
    const option = document.createElement("option");
    option.value = entry.name;
    option.textContent = `${entry.name} (${entry.languages.join(", ")})`;
    ruleSelect.appendChild(option);
  }
  ruleSelect.value = selectedRule;
}

function loadInitialStateFromUrl() {
  const params = new URLSearchParams(window.location.search);
  const pathParam = params.get("path");
  const codeParam = params.get("code");
  const configParam = params.get("config");
  const ruleParam = params.get("rule");

  if (pathParam !== null || codeParam !== null) {
    pathInput.value = pathParam || "src/example.py";
    codeEditor.value = codeParam || "";
    configEditor.value = configParam || "";
  } else {
    const preset = PRESETS["python-test"];
    pathInput.value = preset.path;
    codeEditor.value = preset.code;
    configEditor.value = preset.config;
  }

  if (ruleParam) {
    selectedRule = ruleParam;
    activeTab = "explain";
  }
}

presetSelect.addEventListener("change", () => {
  const preset = PRESETS[presetSelect.value];
  if (!preset) {
    return;
  }
  pathInput.value = preset.path;
  codeEditor.value = preset.code;
  configEditor.value = preset.config;
  runLint();
});

pathInput.addEventListener("input", runLint);
codeEditor.addEventListener("input", runLint);
configEditor.addEventListener("input", runLint);

ruleSelect.addEventListener("change", () => {
  selectedRule = ruleSelect.value;
  renderActiveView();
});

for (const button of tabButtons) {
  button.addEventListener("click", () => {
    switchTab(button.dataset.tab);
  });
}

shareButton.addEventListener("click", async () => {
  const params = new URLSearchParams();
  params.set("path", pathInput.value);
  params.set("code", codeEditor.value);
  if (configEditor.value.trim()) {
    params.set("config", configEditor.value);
  }
  const shareUrl = `${window.location.origin}${window.location.pathname}?${params.toString()}`;
  window.history.replaceState(null, "", `?${params.toString()}`);
  try {
    await navigator.clipboard.writeText(shareUrl);
    shareButton.textContent = "Copied!";
    setTimeout(() => {
      shareButton.textContent = "Copy Share Link";
    }, 1500);
  } catch {
    shareButton.textContent = "URL Updated";
  }
});

loadInitialStateFromUrl();

loadOmnicoach("./omnicoach_playground.wasm")
  .then((loadedClient) => {
    client = loadedClient;
    populateRuleCatalog();
    switchTab(activeTab);
    runLint();
  })
  .catch((error) => {
    setStatusBadge("WASM Load Error", "err");
    renderCards({ ok: false, error: String(error) });
  });
