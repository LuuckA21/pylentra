import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("@tauri-apps/plugin-log", () => ({ warn: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));

import { warn } from "@tauri-apps/plugin-log";
import { relaunch } from "@tauri-apps/plugin-process";
import { setLang } from "../lib/i18n";
import { updates } from "../lib/stores/updates.svelte";
import UpdateBanner from "./UpdateBanner.svelte";

beforeEach(() => {
  vi.resetAllMocks();
  updates.available = null;
  setLang("en");
});

const withUpdate = (
  downloadAndInstall = vi.fn().mockResolvedValue(undefined),
) => {
  updates.available = { version: "1.2.3", downloadAndInstall } as never;
  return downloadAndInstall;
};

test("shows banner when an update is available", async () => {
  const downloadAndInstall = withUpdate();
  render(UpdateBanner);

  expect(screen.getByText("Version 1.2.3 available")).toBeTruthy();
  await fireEvent.click(
    screen.getByRole("button", { name: "Install and restart" }),
  );
  await waitFor(() => expect(relaunch).toHaveBeenCalled());
  expect(downloadAndInstall).toHaveBeenCalled();
});

test("banner appears when the store is set later", async () => {
  render(UpdateBanner);
  expect(screen.queryByRole("button")).toBeNull();
  withUpdate();
  expect(await screen.findByText("Version 1.2.3 available")).toBeTruthy();
});

test("a failed install warns and re-enables the button", async () => {
  withUpdate(vi.fn().mockRejectedValue(new Error("disk full")));
  render(UpdateBanner);
  await fireEvent.click(
    screen.getByRole("button", { name: "Install and restart" }),
  );
  await waitFor(() =>
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("disk full")),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Install and restart" }),
    ).toHaveProperty("disabled", false),
  );
});
