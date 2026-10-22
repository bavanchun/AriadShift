import type { Metadata } from "next";
import { REPOSITORY_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Download",
  description:
    "Install the AriadShift command-line tool and MCP server on macOS, Linux and Windows.",
  alternates: { canonical: "/download" },
};

const RELEASE = `${REPOSITORY_URL}/releases/latest/download`;

const CHANNELS = [
  {
    title: "Homebrew (macOS and Linux)",
    command: "brew tap bavanchun/homebrew-tap\nbrew install ashift",
    note: "Installs Pandoc too, which DOCX and EPUB routes need.",
  },
  {
    title: "Shell installer (macOS and Linux)",
    command: `curl --proto '=https' --tlsv1.2 -LsSf ${RELEASE}/ariad-cli-installer.sh | sh`,
  },
  {
    title: "PowerShell installer (Windows)",
    command: `irm ${RELEASE}/ariad-cli-installer.ps1 | iex`,
  },
  {
    title: "WinGet (Windows)",
    command: "winget install VChun.AriadShift",
    note: "Available once the package submission is accepted.",
  },
  {
    title: "Cargo",
    command: "cargo binstall ariad-cli\n# or build from source\ncargo install ariad-cli",
    note: "Available once the crates are published to crates.io.",
  },
] as const;

export default function DownloadPage() {
  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-3">
        <h1 className="text-4xl font-bold tracking-tight">Download AriadShift</h1>
        <p className="max-w-2xl text-lg text-muted-foreground">
          The <code>ashift</code> command converts Markdown, HTML, DOCX and EPUB on your computer
          and runs an MCP server for AI agents. DOCX and EPUB routes use{" "}
          <a className="underline" href="https://github.com/jgm/pandoc/releases">
            Pandoc 3.12 or newer
          </a>{" "}
          as a separate program.
        </p>
      </header>
      <ul className="flex flex-col gap-6">
        {CHANNELS.map((channel) => (
          <li key={channel.title} className="flex flex-col gap-2">
            <h2 className="text-lg font-semibold">{channel.title}</h2>
            <pre className="overflow-x-auto rounded-lg bg-muted p-4 text-sm">
              <code>{channel.command}</code>
            </pre>
            {"note" in channel && <p className="text-sm text-muted-foreground">{channel.note}</p>}
          </li>
        ))}
      </ul>
      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold">Then</h2>
        <pre className="overflow-x-auto rounded-lg bg-muted p-4 text-sm">
          <code>{"ashift convert notes.md --to docx\nashift mcp --allow-dir ~/Documents"}</code>
        </pre>
        <p className="text-sm text-muted-foreground">
          Archives for every platform are on{" "}
          <a className="underline" href={`${REPOSITORY_URL}/releases`}>
            GitHub Releases
          </a>
          .
        </p>
      </section>
      <section className="flex flex-col gap-2 rounded-xl border border-border bg-card p-5">
        <h2 className="text-lg font-semibold">Desktop app</h2>
        <p className="text-sm text-muted-foreground">
          A desktop app for macOS, Windows and Linux with batch conversion and folder watching is on
          the way. It will be listed here when the first release is out.
        </p>
      </section>
    </div>
  );
}
