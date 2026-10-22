import { afterEach, expect, test } from "vitest";
import { CORE_CODES } from "./codes";
import { en } from "./en";
import { it } from "./it";
import { detectLang, errorMessage, setLang, t } from "./index";

afterEach(() => setLang("en"));

test("en and it have identical keys", () =>
  expect(Object.keys(it).sort()).toEqual(Object.keys(en).sort()));

test("every core error code has a message", () => {
  for (const c of CORE_CODES) expect(en).toHaveProperty(`error.${c}`);
  expect(en).toHaveProperty("error.config.recovered_corrupt");
  expect(en).toHaveProperty("error.unknown");
});

test("detectLang", () => {
  expect(detectLang("it-IT")).toBe("it");
  expect(detectLang("de-DE")).toBe("en");
});

test("interpolation", () =>
  expect(t("error.network.timeout", { host: "h", port: 22 })).toContain(
    "h:22",
  ));

test("missing param leaves the placeholder", () =>
  expect(t("error.network.dns")).toContain("{host}"));

test("t follows the current language", () => {
  setLang("it");
  expect(t("error.cancelled")).toBe(it["error.cancelled"]);
});

test("unknown code falls back", () =>
  expect(errorMessage({ code: "nope", params: {} })).toBe(en["error.unknown"]));

test("errorMessage translates known codes with params", () =>
  expect(
    errorMessage({ code: "forward.port_in_use", params: { port: "80" } }),
  ).toContain("80"));
