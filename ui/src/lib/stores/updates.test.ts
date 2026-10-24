import { beforeEach, expect, test, vi } from "vitest";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));

import { check } from "@tauri-apps/plugin-updater";
import { checkForUpdate, updates } from "./updates.svelte";

beforeEach(() => {
  vi.resetAllMocks();
  updates.available = null;
  updates.checking = false;
});

test("checkForUpdate stores the available update", async () => {
  const update = { version: "1.2.3" };
  vi.mocked(check).mockResolvedValue(update as never);
  expect(await checkForUpdate()).toBe(update);
  expect(updates.available?.version).toBe("1.2.3");
  expect(updates.checking).toBe(false);
});

test("no update leaves available null", async () => {
  vi.mocked(check).mockResolvedValue(null);
  expect(await checkForUpdate()).toBeNull();
  expect(updates.available).toBeNull();
});

test("a failed check rejects and clears checking", async () => {
  vi.mocked(check).mockRejectedValue(new Error("offline"));
  await expect(checkForUpdate()).rejects.toThrow("offline");
  expect(updates.checking).toBe(false);
});
