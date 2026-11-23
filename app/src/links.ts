// Public links shown in the app.
import type { Lang } from "./i18n";

export const GITHUB_URL = "https://github.com/Shupian0510/Ludonimbus";

/** Community chat; the first-run guide and Settings show it once this is set. */
export const DISCORD_URL = "";

const DOCS = "https://shupian0510.github.io/Ludonimbus/";

/** A page of the documentation site in the interface language (Chinese at the root, English under /en/). */
export function docsUrl(lang: Lang, path = ""): string {
  return lang === "en" ? `${DOCS}en/${path}` : `${DOCS}${path}`;
}
