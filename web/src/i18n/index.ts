// Lightweight i18n. Keys live in `~/i18n/{ja,en}.json`.

import en from "./en.json";
import ja from "./ja.json";
import { createSignal } from "solid-js";

export type Locale = "en" | "ja";
type Dict = Record<string, string>;

const dicts: Record<Locale, Dict> = {
  en: en as Dict,
  ja: ja as Dict,
};

const initial: Locale =
  (typeof navigator !== "undefined" &&
    navigator.language?.toLowerCase().startsWith("ja"))
    ? "ja"
    : "en";

const [locale, setLocale] = createSignal<Locale>(initial);

export function t(key: string): string {
  const d = dicts[locale()];
  return d[key] ?? key;
}

export { locale, setLocale };
