import type { Metadata } from "next";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { pageAlternates } from "@/i18n/metadata";
import { REPOSITORY_URL } from "@/lib/site";

const RELEASE = `${REPOSITORY_URL}/releases/latest/download`;

/** Install commands are never translated; each channel's title and note are messages. */
const CHANNELS = [
  {
    key: "homebrew",
    command: "brew tap bavanchun/tap && brew trust --tap bavanchun/tap && brew install ashift",
    note: true,
  },
  {
    key: "shell",
    command: `curl --proto '=https' --tlsv1.2 -LsSf ${RELEASE}/ariad-cli-installer.sh | sh`,
    note: false,
  },
  {
    key: "powershell",
    command: `irm ${RELEASE}/ariad-cli-installer.ps1 | iex`,
    note: false,
  },
  { key: "winget", command: "winget install VChun.AriadShift", note: true },
  {
    key: "cargo",
    command: "cargo binstall ariad-cli\n# or build from source\ncargo install ariad-cli",
    note: true,
  },
] as const;

export async function generateMetadata({
  params,
}: PageProps<"/[locale]/download">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.download" });
  return {
    title: t("meta_title"),
    description: t("meta_description"),
    alternates: pageAlternates(locale, "/download"),
  };
}

export default async function DownloadPage({ params }: PageProps<"/[locale]/download">) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations({ locale, namespace: "web.download" });
  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-3">
        <h1 className="text-4xl font-bold tracking-tight">{t("title")}</h1>
        <p className="max-w-2xl text-lg text-muted-foreground">
          {t.rich("lede", {
            code: (chunks) => <code>{chunks}</code>,
            pandoc: (chunks) => (
              <a className="underline" href="https://github.com/jgm/pandoc/releases">
                {chunks}
              </a>
            ),
          })}
        </p>
      </header>
      <ul className="flex flex-col gap-6">
        {CHANNELS.map((channel) => (
          <li key={channel.key} className="flex flex-col gap-2">
            <h2 className="text-lg font-semibold">{t(`channel.${channel.key}.title`)}</h2>
            <pre className="overflow-x-auto rounded-lg bg-muted p-4 text-sm" dir="ltr">
              <code>{channel.command}</code>
            </pre>
            {channel.note && (
              <p className="text-sm text-muted-foreground">{t(`channel.${channel.key}.note`)}</p>
            )}
          </li>
        ))}
      </ul>
      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold">{t("then_heading")}</h2>
        <pre className="overflow-x-auto rounded-lg bg-muted p-4 text-sm" dir="ltr">
          <code>{"ashift convert notes.md --to docx\nashift mcp --allow-dir ~/Documents"}</code>
        </pre>
        <p className="text-sm text-muted-foreground">
          {t.rich("releases", {
            link: (chunks) => (
              <a className="underline" href={`${REPOSITORY_URL}/releases`}>
                {chunks}
              </a>
            ),
          })}
        </p>
      </section>
      <section className="flex flex-col gap-2 rounded-xl border border-border bg-card p-5">
        <h2 className="text-lg font-semibold">{t("desktop.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("desktop.body")}</p>
      </section>
    </div>
  );
}
