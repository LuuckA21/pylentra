import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: { settingsSave: vi.fn(), logDir: vi.fn() },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath: vi.fn() }));

import { openPath } from "@tauri-apps/plugin-opener";
import { api } from "../lib/api";
import { setLang, t } from "../lib/i18n";
import { settings } from "../lib/stores/settings.svelte";
import Settings from "./Settings.svelte";

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.settingsSave).mockResolvedValue();
  Object.assign(settings, {
    theme: "System",
    font_family: "monospace",
    font_size: 14,
    language: null,
  });
  setLang("en");
});

const setup = () => render(Settings, { onclose: vi.fn() });

test("theme sets data-theme on html", async () => {
  setup();
  await fireEvent.change(screen.getByLabelText("Theme"), {
    target: { value: "Dark" },
  });
  await waitFor(() =>
    expect(document.documentElement.dataset.theme).toBe("dark"),
  );
  expect(api.settingsSave).toHaveBeenCalledWith(
    expect.objectContaining({ theme: "Dark" }),
  );
});

test("font size outside 8..32 rejected", async () => {
  setup();
  const size = screen.getByLabelText("Font size");
  for (const bad of ["7", "33", "", "12.5"]) {
    await fireEvent.change(size, { target: { value: bad } });
    expect(screen.getByText("Enter a size from 8 to 32.")).toBeTruthy();
  }
  expect(api.settingsSave).not.toHaveBeenCalled();
  await fireEvent.change(size, { target: { value: "20" } });
  await waitFor(() =>
    expect(api.settingsSave).toHaveBeenCalledWith(
      expect.objectContaining({ font_size: 20 }),
    ),
  );
  expect(screen.queryByText("Enter a size from 8 to 32.")).toBeNull();
  expect(settings.font_size).toBe(20);
});

test("language change re-renders labels", async () => {
  setup();
  await fireEvent.change(screen.getByLabelText("Language"), {
    target: { value: "it" },
  });
  await waitFor(() => expect(screen.getByLabelText("Lingua")).toBeTruthy());
  expect(t("common.cancel")).toBe("Annulla");
  expect(api.settingsSave).toHaveBeenCalledWith(
    expect.objectContaining({ language: "it" }),
  );
  await fireEvent.change(screen.getByLabelText("Lingua"), {
    target: { value: "" },
  });
  await waitFor(() => expect(settings.language).toBeNull());
});

test("open log folder opens the log dir", async () => {
  vi.mocked(api.logDir).mockResolvedValue("/logs");
  setup();
  await fireEvent.click(
    screen.getByRole("button", { name: "Open log folder" }),
  );
  await waitFor(() => expect(openPath).toHaveBeenCalledWith("/logs"));
});
