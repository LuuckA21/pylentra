import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({ api: { sftpCancel: vi.fn() } }));
vi.mock("../lib/stores/sessions.svelte", () => ({ dismissTransfer: vi.fn() }));

import { api } from "../lib/api";
import { initialPane, type PaneState } from "../lib/sessionReducer";
import { dismissTransfer } from "../lib/stores/sessions.svelte";
import TransferList from "./TransferList.svelte";

const pane = (transfers: PaneState["transfers"]): PaneState => ({
  ...initialPane("t1", "c1"),
  sessionId: "s1",
  phase: "ready",
  transfers,
});

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.sftpCancel).mockResolvedValue();
});

test("a running transfer shows its bar, and Cancel stops it", async () => {
  render(TransferList, {
    pane: pane({
      4: {
        name: "site",
        state: {
          type: "Running",
          done_bytes: 30,
          total_bytes: 120,
          files_done: 1,
          current: "site/a.css",
        },
      },
    }),
  });
  expect(screen.getByText("site")).toBeTruthy();
  const bar = screen.getByRole("progressbar") as HTMLProgressElement;
  expect([bar.value, bar.max]).toEqual([30, 120]);
  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(api.sftpCancel).toHaveBeenCalledWith("s1", 4);
});

test("a finished transfer sums up, lists what it skipped, and × removes it", async () => {
  render(TransferList, {
    pane: pane({
      5: {
        name: "photos",
        state: {
          type: "Finished",
          files: 12,
          skipped: [
            { path: "photos/link", reason: { type: "Symlink" } },
            { path: "photos/a.jpg", reason: { type: "Exists" } },
            {
              path: "photos/b.jpg",
              reason: {
                type: "Error",
                error: {
                  code: "sftp.permission_denied",
                  params: { path: "b.jpg" },
                },
              },
            },
          ],
        },
      },
      6: { name: "one", state: { type: "Finished", files: 1, skipped: [] } },
    }),
  });
  expect(screen.getByText("12 files, 3 skipped")).toBeTruthy();
  expect(screen.getByText("1 file")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  expect(screen.getByText(/photos\/link/).textContent).toContain(
    "symbolic link",
  );
  expect(screen.getByText(/photos\/a\.jpg/).textContent).toContain(
    "already exists",
  );
  expect(screen.getByText(/photos\/b\.jpg/).textContent).toContain(
    "Permission denied: b.jpg",
  );
  await fireEvent.click(screen.getAllByRole("button", { name: "Dismiss" })[0]);
  expect(dismissTransfer).toHaveBeenCalledWith("t1", 5);
});

test("a failed transfer shows its error; a queued one can be cancelled", () => {
  render(TransferList, {
    pane: pane({
      1: {
        name: "big.iso",
        state: {
          type: "Failed",
          error: { code: "sftp.unavailable", params: {} },
          files: 0,
          skipped: [],
        },
      },
      2: {
        name: "next",
        state: { type: "Queued", direction: "Down", name: "next" },
      },
    }),
  });
  expect(screen.getByText("The server does not offer SFTP.")).toBeTruthy();
  expect(screen.getAllByRole("button", { name: "Cancel" })).toHaveLength(1);
});
