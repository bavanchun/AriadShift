// The GPL Pandoc reactor is fetched as a separate, unmodified file and called over WASI.
import { File, OpenFile, PreopenDirectory, WASI } from "@bjorn3/browser_wasi_shim";
import { pandocUrl } from "../../engine-assets.mjs";
import { engineResponse } from "./wasm-cache";

type Reactor = WebAssembly.Instance & {
  exports: {
    memory: WebAssembly.Memory;
    malloc(size: number): number;
    __wasm_call_ctors(): void;
    hs_init_with_rtsopts(argc: number, argv: number): void;
    convert(options: number, length: number): void;
  };
};

export class Pandoc {
  #instance: Reactor | null = null;
  #files = new Map<string, File>();
  get memoryBytes() {
    return this.#instance?.exports.memory.buffer.byteLength ?? 0;
  }

  async #ready() {
    if (this.#instance) return this.#instance;
    if (typeof WebAssembly.compile !== "function") {
      throw {
        code: "engine_unavailable",
        message: "This browser cannot run Pandoc WebAssembly. Open this route in the desktop app.",
      };
    }
    const wasi = new WASI(
      ["pandoc.wasm"],
      [],
      [
        new OpenFile(new File([])),
        new OpenFile(new File([])),
        new OpenFile(new File([])),
        new PreopenDirectory("/", this.#files),
      ],
      { debug: false },
    );
    const response = await engineResponse(pandocUrl);
    const module = await WebAssembly.compile(await response.arrayBuffer());
    const instance = (await WebAssembly.instantiate(module, {
      wasi_snapshot_preview1: wasi.wasiImport,
    })) as Reactor;
    wasi.initialize(instance);
    instance.exports.__wasm_call_ctors();
    const allocate = (bytes: Uint8Array) => {
      const ptr = instance.exports.malloc(bytes.length);
      new Uint8Array(instance.exports.memory.buffer, ptr, bytes.length).set(bytes);
      return ptr;
    };
    const args = ["pandoc.wasm", "+RTS", "-H64m", "-RTS"].map((arg) =>
      allocate(new TextEncoder().encode(`${arg}\0`)),
    );
    const argv = instance.exports.malloc((args.length + 1) * 4);
    const argcPtr = instance.exports.malloc(4);
    const argvPtr = instance.exports.malloc(4);
    const view = new DataView(instance.exports.memory.buffer);
    args.forEach((ptr, index) => {
      view.setUint32(argv + index * 4, ptr, true);
    });
    view.setUint32(argv + args.length * 4, 0, true);
    view.setUint32(argcPtr, args.length, true);
    view.setUint32(argvPtr, argv, true);
    instance.exports.hs_init_with_rtsopts(argcPtr, argvPtr);
    this.#instance = instance;
    return instance;
  }

  async convert(bytes: Uint8Array, from: string, to: string) {
    const instance = await this.#ready();
    const reading = from !== "json";
    const output = new File([]);
    const stderr = new File([]);
    const warnings = new File([]);
    const media = new File([]);
    this.#files.clear();
    this.#files.set("input", new File(bytes, { readonly: true }));
    this.#files.set("output", output);
    this.#files.set("stdin", new File([]));
    this.#files.set("stdout", new File([]));
    this.#files.set("stderr", stderr);
    this.#files.set("warnings", warnings);
    if (reading) this.#files.set("media.zip", media);
    const options = new TextEncoder().encode(
      JSON.stringify({
        from,
        to,
        "input-files": ["input"],
        "output-file": "output",
        ...(reading ? { "extract-media": "media.zip" } : {}),
        standalone: true,
      }),
    );
    // The reactor takes ownership and frees this allocation after parsing its options.
    const pointer = instance.exports.malloc(options.length);
    new Uint8Array(instance.exports.memory.buffer, pointer, options.length).set(options);
    try {
      instance.exports.convert(pointer, options.length);
      if (!output.data.length)
        throw {
          code: "invalid_input",
          message:
            "Pandoc could not convert this document. Check that the file is readable in its original application.",
        };
      return {
        bytes: output.data.slice(),
        media: media.data.slice(),
        warnings:
          warnings.data.length && JSON.parse(new TextDecoder().decode(warnings.data)).length
            ? [
                {
                  code: "pandoc_warning",
                  message: "Pandoc reported conversion warnings; some formatting may have changed.",
                },
              ]
            : [],
      };
    } finally {
      this.#files.clear();
    }
  }
}
