// Tesseract language packs offered by the OCR studio. Vietnamese leads the list.
export interface OcrLanguage {
  /** Tesseract code sent to the engine. */
  code: string;
  /** English name, also searchable. */
  name: string;
  /** Name in the language itself, also searchable. */
  native: string;
}

export const OCR_LANGUAGES: readonly OcrLanguage[] = [
  { code: "vie", name: "Vietnamese", native: "Tiếng Việt" },
  { code: "eng", name: "English", native: "English" },
  { code: "fra", name: "French", native: "Français" },
  { code: "deu", name: "German", native: "Deutsch" },
  { code: "spa", name: "Spanish", native: "Español" },
  { code: "ita", name: "Italian", native: "Italiano" },
  { code: "por", name: "Portuguese", native: "Português" },
  { code: "nld", name: "Dutch", native: "Nederlands" },
  { code: "pol", name: "Polish", native: "Polski" },
  { code: "tur", name: "Turkish", native: "Türkçe" },
  { code: "rus", name: "Russian", native: "Русский" },
  { code: "ukr", name: "Ukrainian", native: "Українська" },
  { code: "jpn", name: "Japanese", native: "日本語" },
  { code: "kor", name: "Korean", native: "한국어" },
  { code: "chi_sim", name: "Chinese (Simplified)", native: "简体中文" },
  { code: "chi_tra", name: "Chinese (Traditional)", native: "繁體中文" },
  { code: "ara", name: "Arabic", native: "العربية" },
  { code: "heb", name: "Hebrew", native: "עברית" },
  { code: "hin", name: "Hindi", native: "हिन्दी" },
  { code: "tha", name: "Thai", native: "ไทย" },
  { code: "ind", name: "Indonesian", native: "Bahasa Indonesia" },
];

export const DEFAULT_OCR_LANGUAGES = ["vie", "eng"];

export function languageName(code: string): string {
  return OCR_LANGUAGES.find((language) => language.code === code)?.name ?? code;
}

/** Case- and accent-insensitive match on the code, English name and native name. */
export function filterLanguages(query: string): OcrLanguage[] {
  const fold = (text: string) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
  const needle = fold(query.trim());
  if (!needle) return [...OCR_LANGUAGES];
  return OCR_LANGUAGES.filter((language) =>
    [language.code, language.name, language.native].some((text) => fold(text).includes(needle)),
  );
}
