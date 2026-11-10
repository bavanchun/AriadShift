import { allLocales, DEFAULT_LOCALE } from "@/i18n/locales";

// Static entry page for `/`. The site is statically exported, so there is no request-time
// locale negotiation: this page sends visitors to the best supported language from the
// browser's language list and falls back to English. It is only a convenience, never the
// only way to reach a locale: every language also has its own link below, and the refresh
// tag covers visitors without scripting.
export default function RootPage() {
  const locales = allLocales().filter((locale) => locale.published);
  const supported = locales.map((locale) => locale.tag);
  const script = `(function(){var s=${JSON.stringify(supported)};var d=${JSON.stringify(DEFAULT_LOCALE)};var l=navigator.languages&&navigator.languages.length?navigator.languages:[navigator.language];var lower=s.map(function(x){return x.toLowerCase()});for(var i=0;i<l.length;i++){var w=String(l[i]).toLowerCase();var k=lower.indexOf(w);if(k<0){var b=w.split("-")[0];k=lower.indexOf(b);if(k<0){for(var j=0;j<lower.length;j++){if(lower[j].split("-")[0]===b){k=j;break}}}}if(k>=0){d=s[k];break}}location.replace("/"+d+location.search+location.hash)})()`;
  return (
    <html lang="en">
      <head>
        <title>AriadShift</title>
        <meta httpEquiv="refresh" content={`1;url=/${DEFAULT_LOCALE}`} />
        <meta name="robots" content="noindex" />
        <script dangerouslySetInnerHTML={{ __html: script }} />
      </head>
      <body
        style={{
          fontFamily: "system-ui, sans-serif",
          margin: "3rem auto",
          maxWidth: "40rem",
          padding: "0 1rem",
        }}
      >
        <h1>AriadShift</h1>
        <ul style={{ columns: "12rem", listStyle: "none", padding: 0 }}>
          {locales.map((locale) => (
            <li key={locale.tag}>
              <a href={`/${locale.tag}`} lang={locale.tag} hrefLang={locale.tag}>
                {locale.name}
              </a>
            </li>
          ))}
        </ul>
      </body>
    </html>
  );
}
