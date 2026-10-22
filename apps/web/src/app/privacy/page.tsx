import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Privacy",
  description: "What the AriadShift website does with your documents: nothing leaves your device.",
  alternates: { canonical: "/privacy" },
};

export default function PrivacyPage() {
  return (
    <article className="flex max-w-3xl flex-col gap-6">
      <h1 className="text-4xl font-bold tracking-tight">Privacy</h1>
      <p className="text-lg text-muted-foreground">
        The converter on this website runs entirely in your browser. This page explains what that
        means in practice.
      </p>
      <section className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">Your documents stay on your device</h2>
        <p>
          When you choose a file, the browser copies it into its origin private file system, a
          storage area that only this website can read and that is not part of your regular files.
          The conversion runs in a background worker using WebAssembly. The file is never sent to a
          server, and the converted file is written to the same private storage, from which you
          download it. Each open tab keeps its files in its own folder there.
        </p>
        <p>
          If your browser cannot provide that storage, the files are held in the worker's memory
          instead and disappear when you close the tab.
        </p>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">Deleting your files</h2>
        <p>
          “Start over and delete files” removes the input and output from the private storage at
          once. Files of a tab you close without starting over stay in that storage until you next
          open the converter, which deletes them on start. Clearing this site's data in your browser
          settings removes them as well.
        </p>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">No tracking</h2>
        <p>
          The website sets no cookies, loads no analytics or advertising scripts and makes no
          requests to other sites. A service worker caches the converter so it keeps working
          offline; it stores only the website's own code. Your theme choice is kept in local
          storage.
        </p>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">Hosting</h2>
        <p>
          Like any website, the server that delivers these pages may keep standard access logs (IP
          address, time, requested URL). Those requests never include your documents.
        </p>
      </section>
    </article>
  );
}
