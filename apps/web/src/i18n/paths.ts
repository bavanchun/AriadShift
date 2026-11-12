// URL helpers shared by server and client code. Every page lives under /<locale>/.

/** `/vi` + `/app` -> `/vi/app`; the home page is `/vi`. */
export function localePath(locale: string, path: string): string {
  return path === "/" || path === "" ? `/${locale}` : `/${locale}${path}`;
}

/** Splits `/vi/app` into the locale segment and the page path (`/app`). */
export function splitLocalePath(pathname: string): { locale: string | null; path: string } {
  const [, first = "", ...rest] = pathname.split("/");
  if (first === "") {
    return { locale: null, path: "/" };
  }
  return { locale: first, path: `/${rest.join("/")}`.replace(/(.)\/$/, "$1") };
}
