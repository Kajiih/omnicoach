// Stateless WebAssembly client for omnicoach.
// Works in browsers (via fetch) and in Node/Deno/Bun/Edge runtimes (via bytes/BufferSource).

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

/**
 * Loads and instantiates the `omnicoach_playground.wasm` module.
 *
 * @param {string | URL | BufferSource | WebAssembly.Module} source
 *   URL to `omnicoach_playground.wasm` (default `"./omnicoach_playground.wasm"`) or raw WASM bytes.
 */
export async function loadOmnicoach(source = "./omnicoach_playground.wasm") {
  let instance;
  if (typeof source === "string" || source instanceof URL) {
    const response = await fetch(source);
    if (!response.ok) {
      throw new Error(`Failed to fetch WASM module (${response.status})`);
    }
    if (typeof WebAssembly.instantiateStreaming === "function") {
      try {
        const instantiated = await WebAssembly.instantiateStreaming(response.clone(), {});
        instance = instantiated.instance;
      } catch {
        const bytes = await response.arrayBuffer();
        const instantiated = await WebAssembly.instantiate(bytes, {});
        instance = instantiated.instance;
      }
    } else {
      const bytes = await response.arrayBuffer();
      const instantiated = await WebAssembly.instantiate(bytes, {});
      instance = instantiated.instance;
    }
  } else if (source instanceof WebAssembly.Module) {
    instance = await WebAssembly.instantiate(source, {});
  } else {
    const instantiated = await WebAssembly.instantiate(source, {});
    instance = instantiated.instance;
  }

  const exports = instance.exports;

  function sendRequest(payloadObject) {
    const requestBytes = textEncoder.encode(JSON.stringify(payloadObject));
    const inputPointer = exports.prepare_input(requestBytes.byteLength);
    const memoryBytes = new Uint8Array(exports.memory.buffer);
    memoryBytes.set(requestBytes, inputPointer);

    const outputLength = exports.execute_request();
    const outputPointer = exports.output_pointer();
    const responseView = new Uint8Array(exports.memory.buffer, outputPointer, outputLength);
    const responseJson = textDecoder.decode(responseView);
    return JSON.parse(responseJson);
  }

  return {
    request: sendRequest,
    lint({ path, code, config = "" }) {
      return sendRequest({ action: "lint", path, code, config });
    },
    explain(rule, config = "") {
      return sendRequest({ action: "explain", rule, config });
    },
    catalog() {
      return sendRequest({ action: "catalog" });
    },
  };
}
