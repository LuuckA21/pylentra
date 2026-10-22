import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));

import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { setLang } from "../lib/i18n";
import UpdateBanner from "./UpdateBanner.svelte";

beforeEach(() => {
  vi.resetAllMocks();
  setLang("en");
});

test("shows banner when update available", async () => {
  const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
  vi.mocked(check).mockResolvedValue({
    version: "1.2.3",
    downloadAndInstall,
  } as never);
  render(UpdateBanner);

  expect(await screen.findByText("Version 1.2.3 available")).toBeTruthy();
  await fireEvent.click(
    screen.getByRole("button", { name: "Install and restart" }),
  );
  await waitFor(() => expect(relaunch).toHaveBeenCalled());
  expect(downloadAndInstall).toHaveBeenCalled();
});

test("no banner and no throw when check rejects", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.mocked(check).mockRejectedValue(new Error("offline"));
  const { container } = render(UpdateBanner);

  await waitFor(() => expect(warn).toHaveBeenCalled());
  expect(container.querySelector(".update")).toBeNull();
  expect(screen.queryByRole("button")).toBeNull();
});

test("no banner when up to date", async () => {
  vi.mocked(check).mockResolvedValue(null);
  render(UpdateBanner);
  await waitFor(() => expect(check).toHaveBeenCalled());
  expect(screen.queryByRole("button")).toBeNull();
});
