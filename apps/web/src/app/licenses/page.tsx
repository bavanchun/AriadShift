import type { Metadata } from "next";
import { pandocSourceInventory } from "../../../pandoc-source-inventory.mjs";

export const metadata: Metadata = {
  title: "Licences",
  description: "Licences and source for AriadShift and its browser engines.",
  alternates: { canonical: "/licenses" },
};

export default function LicensesPage() {
  return (
    <div className="flex flex-col gap-6">
      <h1 className="text-4xl font-bold">Licences</h1>
      <p>
        AriadShift is licensed under Apache-2.0. Its browser engines and embedded fonts retain their
        own licences.
      </p>
      <section className="flex flex-col gap-2">
        <h2 className="text-2xl font-semibold">Pandoc {pandocSourceInventory.version}</h2>
        <p>
          The official unmodified pandoc.wasm is aggregated as a separate file, loaded only for DOCX
          or EPUB routes and invoked through WASI in a worker. It is never linked into AriadShift
          code. Pandoc is licensed under {pandocSourceInventory.license}.
        </p>
        <p>
          We must keep the corresponding source available for as long as we ship this pandoc.wasm
          build. The source links below must remain reachable; if upstream stops serving them, we
          must provide an accessible copy of the corresponding source.
        </p>
        <p>
          <a className="underline" href="wasm/pandoc/COPYING.md">
            GPL licence
          </a>
          {" · "}
          <a className="underline" href="wasm/pandoc/COPYRIGHT">
            Copyright notices
          </a>
          {" · "}
          <a className="underline" href="wasm/pandoc/SOURCE.txt">
            Source record
          </a>
          {" · "}
          <a className="underline" href={pandocSourceInventory.sourceUrls.tagTree}>
            Corresponding source
          </a>
          {" · "}
          <a className="underline" href={pandocSourceInventory.sourceUrls.commitTree}>
            Release commit ({pandocSourceInventory.gitCommit.slice(0, 12)})
          </a>
          {" · "}
          <a className="underline" href={pandocSourceInventory.sourceUrls.sourceArchive}>
            Source archive
          </a>
        </p>
        <details className="rounded border p-3 text-sm">
          <summary className="cursor-pointer font-medium">
            Build recipe, toolchain, and linked Hackage package inventory
          </summary>
          <div className="mt-3 flex flex-col gap-3">
            <div>
              <h3 className="font-semibold">Upstream build recipe</h3>
              <p className="mt-1">{pandocSourceInventory.recipe.summary}</p>
              <ul className="mt-1 list-disc pl-5">
                {pandocSourceInventory.recipe.files.map((item) => (
                  <li key={item.url}>
                    <a className="underline" href={item.url}>
                      {item.label}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <h3 className="font-semibold">Compiler, WASI libc, and toolchain sources</h3>
              <ul className="mt-1 list-disc pl-5">
                {pandocSourceInventory.toolchain.map((item) => (
                  <li key={item.name}>
                    <code>{item.name}</code> — {item.version} (
                    <a className="underline" href={item.sourceUrl}>
                      source
                    </a>
                    {item.secondaryUrl ? (
                      <>
                        {", "}
                        <a className="underline" href={item.secondaryUrl}>
                          release
                        </a>
                      </>
                    ) : null}
                    )
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <h3 className="font-semibold">
                Linked Haskell and C packages verified in pandoc.wasm (
                {pandocSourceInventory.embeddedPackages.length} Haskell packages,{" "}
                {pandocSourceInventory.embeddedCSources.length} C source bundle)
              </h3>
              <ul className="mt-1 list-disc pl-5">
                {pandocSourceInventory.embeddedPackages.map((pkg) => (
                  <li key={`${pkg.name}-${pkg.version}`}>
                    <code>
                      {pkg.name}-{pkg.version}
                    </code>{" "}
                    (units: <code>{pkg.unitIds.join(", ")}</code>) —{" "}
                    <a className="underline" href={pkg.sourceUrl}>
                      source
                    </a>
                    {pkg.patchUrl ? (
                      <>
                        {" ("}
                        <a className="underline" href={pkg.patchUrl}>
                          wasm patch
                        </a>
                        {")"}
                      </>
                    ) : null}
                  </li>
                ))}
                {pandocSourceInventory.embeddedCSources.map((item) => (
                  <li key={item.name}>
                    <code>{item.name}</code> — {item.version} (
                    <a className="underline" href={item.sourceUrl}>
                      source
                    </a>
                    )
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <h3 className="font-semibold">
                Additional Cabal dependencies (upstream provides no wasm32-wasi freeze file)
              </h3>
              <p className="mt-1">{pandocSourceInventory.unfrozenCabalNote}</p>
              <ul className="mt-1 list-disc pl-5">
                {pandocSourceInventory.unfrozenCabalDependencies.map((item) => (
                  <li key={item.name}>
                    <code>{item.name}</code> ({item.constraint}) —{" "}
                    <a className="underline" href={item.sourceUrl}>
                      source
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </details>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-2xl font-semibold">Typst, fonts and third-party notices</h2>
        <p>
          Typst is licensed under Apache-2.0. PDF output embeds the unmodified Libertinus and Source
          Code Pro fonts under SIL OFL-1.1, and the website uses Plus Jakarta Sans under SIL
          OFL-1.1.
        </p>
        <p>
          The core WebAssembly module includes the unmodified MPL-2.0 crates <code>cssparser</code>{" "}
          0.38.0 (
          <a className="underline" href="https://crates.io/crates/cssparser/0.38.0">
            crates.io
          </a>
          {", "}
          <a className="underline" href="https://github.com/servo/rust-cssparser">
            source
          </a>
          ) and <code>dtoa-short</code> 0.3.5 (
          <a className="underline" href="https://crates.io/crates/dtoa-short/0.3.5">
            crates.io
          </a>
          {", "}
          <a className="underline" href="https://github.com/upsuper/dtoa-short">
            source
          </a>
          ) as transitive dependencies of the <code>ammonia</code> HTML sanitizer. The source code
          of these crates is available under MPL-2.0 from crates.io and their respective
          repositories.
        </p>
        <p>
          <a className="underline" href="notices/NOTICE.txt">
            Full notices and font licences
          </a>
          {" · "}
          <a className="underline" href="notices/THIRD_PARTY_LICENSES.txt">
            Rust and WebAssembly dependency licences
          </a>
          {" · "}
          <a className="underline" href="notices/THIRD_PARTY_JS_LICENSES.txt">
            JavaScript dependency licences
          </a>
        </p>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-2xl font-semibold">Browser WASI shim</h2>
        <p>
          <code>@bjorn3/browser_wasi_shim</code> 0.4.2 is dual-licensed under{" "}
          <code>MIT OR Apache-2.0</code> (<code>package.json</code>{" "}
          <code>author: &quot;bjorn3&quot;</code>,{" "}
          <a className="underline" href="https://github.com/bjorn3/browser_wasi_shim/tree/v0.4.2">
            upstream source v0.4.2
          </a>
          ). The upstream package ships <code>LICENSE-APACHE</code> and <code>LICENSE-MIT</code>{" "}
          without a separate copyright header line in <code>LICENSE-MIT</code>; its verbatim{" "}
          <code>LICENSE-MIT</code> text is reproduced below, and both licence files are included in{" "}
          <a className="underline" href="notices/THIRD_PARTY_JS_LICENSES.txt">
            JavaScript dependency licences
          </a>
          .
        </p>
        <pre className="overflow-x-auto rounded border p-3 text-xs whitespace-pre-wrap">
          {`Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.`}
        </pre>
      </section>
    </div>
  );
}
