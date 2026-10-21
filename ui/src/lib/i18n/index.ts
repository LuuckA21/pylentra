import type { ApiError } from "../bindings/ApiError";
import { en, type MessageKey } from "./en";
import { it } from "./it";
import { current } from "./lang.svelte";

export type { MessageKey };
export type Lang = "en" | "it";

const messages: Record<Lang, Record<MessageKey, string>> = { en, it };

export function detectLang(navLang: string): Lang {
  return navLang.toLowerCase().startsWith("it") ? "it" : "en";
}

export function setLang(lang: Lang): void {
  current.lang = lang;
}

export function t(
  key: MessageKey,
  params: Record<string, string | number> = {},
): string {
  return messages[current.lang][key].replace(/\{(\w+)\}/g, (m, name: string) =>
    name in params ? String(params[name]) : m,
  );
}

export function errorMessage(e: ApiError): string {
  const key = `error.${e.code}`;
  return t(key in en ? (key as MessageKey) : "error.unknown", e.params);
}
