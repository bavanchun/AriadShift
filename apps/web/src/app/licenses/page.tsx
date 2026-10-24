import type { Metadata } from "next";
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
        <h2 className="text-2xl font-semibold">Pandoc 3.12</h2>
        <p>
          The official unmodified pandoc.wasm is aggregated as a separate file, loaded only for DOCX
          or EPUB routes and invoked through WASI in a worker. It is never linked into AriadShift
          code. Pandoc is licensed under GPL-2.0-or-later.
        </p>
        <p>
          <a className="underline" href="/wasm/pandoc/COPYING.md">
            GPL licence
          </a>
          {" · "}
          <a className="underline" href="/wasm/pandoc/COPYRIGHT">
            Copyright notices
          </a>
          {" · "}
          <a className="underline" href="https://github.com/jgm/pandoc/tree/3.12">
            Corresponding source
          </a>
          {" · "}
          <a
            className="underline"
            href="https://github.com/jgm/pandoc/archive/refs/tags/3.12.tar.gz"
          >
            Source archive
          </a>
        </p>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-2xl font-semibold">Typst and fonts</h2>
        <p>
          Typst is licensed under Apache-2.0. PDF output embeds the unmodified Libertinus and Source
          Code Pro fonts under SIL OFL-1.1.
        </p>
        <a className="underline" href="https://github.com/bavanchun/AriadShift/blob/dev/NOTICE">
          Full notices and font licences
        </a>
      </section>
      <section>
        <h2 className="text-2xl font-semibold">Browser WASI shim</h2>
        <p>@bjorn3/browser_wasi_shim 0.4.2 is licensed under Apache-2.0 OR MIT.</p>
      </section>
    </div>
  );
}
