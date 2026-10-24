import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    settingsSave: vi.fn(),
    logDir: vi.fn(),
    knownHostsList: vi.fn(),
    knownHostForget: vi.fn(),
  },
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath: vi.fn() }));

import { getVersion } from "@tauri-apps/api/app";
import { openPath } from "@tauri-apps/plugin-opener";
import { api } from "../lib/api";
import { setLang, t } from "../lib/i18n";
import { settings } from "../lib/stores/settings.svelte";
import Settings from "./Settings.svelte";

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.settingsSave).mockResolvedValue();
  vi.mocked(api.knownHostsList).mockResolvedValue([]);
  vi.mocked(getVersion).mockResolvedValue("0.2.0");
  localStorage.clear();
  Object.assign(settings, {
    theme: "System",
    font_family: "monospace",
    font_size: 14,
    language: null,
    scrollback: 10000,
    default_user: null,
    default_port: 22,
    connect_timeout_secs: 15,
    keepalive_secs: 30,
  });
  setLang("en");
});

const setup = () => render(Settings, { onclose: vi.fn() });
const goTo = (name: string) =>
  fireEvent.click(screen.getByRole("button", { name }));
const saved = () => vi.mocked(api.settingsSave).mock.calls.at(-1)?.[0];

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
  await goTo("Terminal");
  const size = screen.getByLabelText("Size");
  for (const bad of ["7", "33", "", "12.5"]) {
    await fireEvent.change(size, { target: { value: bad } });
    expect(screen.getByText("Enter a whole number from 8 to 32.")).toBeTruthy();
  }
  expect(api.settingsSave).not.toHaveBeenCalled();
  await fireEvent.change(size, { target: { value: "20" } });
  await waitFor(() =>
    expect(api.settingsSave).toHaveBeenCalledWith(
      expect.objectContaining({ font_size: 20 }),
    ),
  );
  expect(screen.queryByText("Enter a whole number from 8 to 32.")).toBeNull();
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

test("shows five categories and switches between them", async () => {
  setup();
  const names = [
    "Appearance",
    "Terminal",
    "Connections",
    "Security",
    "Updates",
  ];
  for (const n of names)
    expect(screen.getByRole("button", { name: n })).toBeTruthy();
  expect(
    screen
      .getByRole("button", { name: "Appearance" })
      .getAttribute("aria-current"),
  ).toBe("page");
  expect(screen.getByLabelText("Theme")).toBeTruthy();
  await goTo("Terminal");
  expect(
    screen
      .getByRole("button", { name: "Terminal" })
      .getAttribute("aria-current"),
  ).toBe("page");
  expect(screen.getByLabelText("Scrollback")).toBeTruthy();
  expect(screen.queryByLabelText("Theme")).toBeNull();
  await waitFor(() => expect(screen.getByText("Pylentra 0.2.0")).toBeTruthy());
});

test("remembers the last category", async () => {
  const first = setup();
  await goTo("Security");
  first.unmount();
  setup();
  expect(
    screen
      .getByRole("button", { name: "Security" })
      .getAttribute("aria-current"),
  ).toBe("page");
  expect(JSON.parse(localStorage.getItem("pylentra.settings.section")!)).toBe(
    "security",
  );
});

test("out of range scrollback shows an error and is not saved", async () => {
  setup();
  await goTo("Terminal");
  await fireEvent.change(screen.getByLabelText("Scrollback"), {
    target: { value: "500" },
  });
  expect(
    screen.getByText("Enter a whole number from 1000 to 100000."),
  ).toBeTruthy();
  expect(api.settingsSave).not.toHaveBeenCalled();
  await fireEvent.change(screen.getByLabelText("Scrollback"), {
    target: { value: "5000" },
  });
  await waitFor(() => expect(saved()).toMatchObject({ scrollback: 5000 }));
});

test("terminal controls save: theme card, cursor, switches", async () => {
  setup();
  await goTo("Terminal");
  await fireEvent.click(screen.getByRole("radio", { name: /Dracula/ }));
  await waitFor(() =>
    expect(saved()).toMatchObject({ terminal_theme: "dracula" }),
  );
  await fireEvent.click(screen.getByLabelText("Bar"));
  await waitFor(() => expect(saved()).toMatchObject({ cursor_style: "bar" }));
  await fireEvent.click(screen.getByRole("switch", { name: "Copy on select" }));
  await waitFor(() => expect(saved()).toMatchObject({ copy_on_select: true }));
});

test("known hosts are listed and can be removed", async () => {
  vi.mocked(api.knownHostsList).mockResolvedValue([
    {
      host: "web.example",
      port: 2222,
      algo: "ssh-ed25519",
      fingerprint: "SHA256:aaa",
    },
    { host: null, port: 22, algo: "ssh-rsa", fingerprint: "SHA256:bbb" },
  ]);
  vi.mocked(api.knownHostForget).mockResolvedValue(1);
  setup();
  await goTo("Security");
  await screen.findByText("web.example:2222");
  expect(screen.getByText("Encrypted entry")).toBeTruthy();
  expect(screen.getAllByRole("button", { name: /^Remove/ })).toHaveLength(1);
  expect(screen.getByText(/~\/\.ssh\/known_hosts/)).toBeTruthy();
  vi.mocked(api.knownHostsList).mockResolvedValue([]);
  await fireEvent.click(
    screen.getByRole("button", { name: "Remove web.example:2222" }),
  );
  await waitFor(() =>
    expect(api.knownHostForget).toHaveBeenCalledWith("web.example", 2222),
  );
  await screen.findByText("No saved host keys.");
});

test("Connections fields save default user, port, timeout and keepalive", async () => {
  setup();
  await goTo("Connections");
  await fireEvent.change(screen.getByLabelText("Default user"), {
    target: { value: " root " },
  });
  await fireEvent.change(screen.getByLabelText("Default port"), {
    target: { value: "2200" },
  });
  await fireEvent.change(screen.getByLabelText("Connect timeout"), {
    target: { value: "30" },
  });
  await fireEvent.change(screen.getByLabelText("Keepalive"), {
    target: { value: "0" },
  });
  await waitFor(() =>
    expect(saved()).toMatchObject({
      default_user: "root",
      default_port: 2200,
      connect_timeout_secs: 30,
      keepalive_secs: 0,
    }),
  );
  await fireEvent.change(screen.getByLabelText("Keepalive"), {
    target: { value: "3" },
  });
  expect(screen.getByText(/Enter 0 \(off\)/)).toBeTruthy();
});

test("a rejected field is shown in its own category", async () => {
  vi.mocked(api.settingsSave).mockRejectedValue({
    code: "config.invalid",
    params: { field: "scrollback", reason: "must be between 1000 and 100000" },
  });
  setup();
  await fireEvent.change(screen.getByLabelText("Theme"), {
    target: { value: "Dark" },
  });
  await screen.findByText(/Invalid value for scrollback/);
  expect(
    screen
      .getByRole("button", { name: "Terminal" })
      .getAttribute("aria-current"),
  ).toBe("page");
  expect(screen.getByLabelText("Scrollback").getAttribute("aria-invalid")).toBe(
    "true",
  );
});
