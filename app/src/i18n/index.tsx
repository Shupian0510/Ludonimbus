// Interface languages. `zh-CN.ts` is the source text; every other language
// file must export the same namespaces with the same keys (TypeScript checks
// this, so a missing or extra entry fails the build). Texts that take values
// are functions. See docs/i18n.md for the glossary and how to add a language.
import { createContext, ReactNode, useContext, useEffect, useMemo, useState } from "react";
import * as en from "./en";
import * as zhCN from "./zh-CN";

export type Messages = typeof zhCN;
export type Lang = "zh-CN" | "en";
/** What the user picked in Settings; "system" follows the Windows display language. */
export type LangPref = "system" | Lang;

export const LANGS: { id: Lang; label: string }[] = [
  { id: "zh-CN", label: "简体中文" },
  { id: "en", label: "English" },
];

const catalogs: Record<Lang, Messages> = { "zh-CN": zhCN, en };

export function systemLang(): Lang {
  const l = (navigator.languages?.[0] ?? navigator.language ?? "").toLowerCase();
  return l.startsWith("zh") ? "zh-CN" : "en";
}

export function resolveLang(pref: string | null | undefined): Lang {
  return pref === "zh-CN" || pref === "en" ? pref : systemLang();
}

export function asPref(v: string | null | undefined): LangPref {
  return v === "zh-CN" || v === "en" ? v : "system";
}

let current: Lang = systemLang();

/** Messages in the current language, for code outside components (formatters, event handlers). */
export function msg(): Messages {
  return catalogs[current];
}

export function currentLang(): Lang {
  return current;
}

export { plural } from "./plural";

type Ctx = { t: Messages; lang: Lang; pref: LangPref; setPref: (p: LangPref) => void };
const I18nCtx = createContext<Ctx | null>(null);

export function I18nProvider({ initial, children }: { initial: LangPref; children: ReactNode }) {
  const [pref, setPref] = useState<LangPref>(initial);
  const lang = resolveLang(pref);
  // Set during render so formatters called while rendering see the new language.
  current = lang;
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
  const value = useMemo(() => ({ t: catalogs[lang], lang, pref, setPref }), [lang, pref]);
  return <I18nCtx.Provider value={value}>{children}</I18nCtx.Provider>;
}

function useCtx(): Ctx {
  const v = useContext(I18nCtx);
  if (!v) throw new Error("I18nProvider missing");
  return v;
}

/** Messages in the current language; the component re-renders when it changes. */
export function useT(): Messages {
  return useCtx().t;
}

export function useI18n(): Ctx {
  return useCtx();
}
