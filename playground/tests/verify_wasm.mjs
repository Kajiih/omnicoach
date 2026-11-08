#!/usr/bin/env node
// Verifies that `omnicoach_playground.wasm` compiles with zero WebAssembly host
// imports and executes end-to-end through `playground/www/omnicoach.js`.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadOmnicoach } from "../www/omnicoach.js";

const currentDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(currentDir, "..", "..");
const explicitPath = process.argv[2];
const targetDir = process.env.CARGO_TARGET_DIR
  ? resolve(process.cwd(), process.env.CARGO_TARGET_DIR)
  : resolve(repoRoot, "target");
const wasmPath = resolve(
  process.cwd(),
  explicitPath ?? resolve(targetDir, "wasm32-unknown-unknown/release/omnicoach_playground.wasm"),
);

const wasmBytes = readFileSync(wasmPath);
const wasmModule = new WebAssembly.Module(wasmBytes);
const imports = WebAssembly.Module.imports(wasmModule);
assert.deepEqual(
  imports,
  [],
  `Expected 0 WebAssembly imports in ${wasmPath}, found ${imports.length}: ${JSON.stringify(imports)}`,
);

const omni = await loadOmnicoach(wasmBytes);
const moduleOmni = await loadOmnicoach(wasmModule);

const pyResult = omni.lint({
  path: "tests/test_order.py",
  code: "import time\n\ndef test_order():\n    time.sleep(1)\n",
  config: "",
});
assert.equal(pyResult.ok, true);
assert.equal(pyResult.language, "python");
assert.equal(pyResult.diagnostics[0]?.rule_name, "sleep-in-tests");

// Exercising Rust syntax parsing and teardown verifies that `ra_ap_syntax` is
// compiled with `--cfg no_salsa_async_drops` and does not spawn a drop thread.
const rustResult = moduleOmni.lint({
  path: "src/lib.rs",
  code: "pub fn compute(price_list: &Vec<u64>) -> u64 { price_list.len() as u64 }\n",
  config: 'ignore = ["jj", "edit-of-described-commit"]\n',
});
assert.equal(rustResult.ok, true, JSON.stringify(rustResult));
assert.equal(rustResult.language, "rust");
assert.equal(rustResult.diagnostics[0]?.rule_name, "type-suffixed-name");

const explainResult = omni.explain("edit-of-described-commit", 'ignore = ["jj"]\n');
assert.equal(explainResult.ok, true, JSON.stringify(explainResult));
assert.match(explainResult.markdown, /# edit-of-described-commit/);
assert.match(explainResult.markdown, /Status: disabled by `ignore = \["jj"\]`/);

const catalogResult = omni.catalog();
assert.equal(catalogResult.ok, true);
assert.ok(Array.isArray(catalogResult.rules) && catalogResult.rules.length > 50);

console.log(
  `Verified ${wasmPath} (${wasmBytes.byteLength} bytes, 0 imports, ${catalogResult.rules.length} code rules).`,
);
