import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    forwardStart: vi.fn(),
    forwardStop: vi.fn(),
    connectionSave: vi.fn(),
  },
}));

import { api } from "../lib/api";
import type { Connection } from "../lib/bindings/Connection";
import type { ForwardRule } from "../lib/bindings/ForwardRule";
import type { ForwardState } from "../lib/bindings/ForwardState";
import { library } from "../lib/stores/library.svelte";
import type { TabState } from "../lib/sessionReducer";
import ForwardPanel from "./ForwardPanel.svelte";

const local: ForwardRule = {
  type: "Local",
  bind_host: "127.0.0.1",
  bind_port: 5432,
  dest_host: "db",
  dest_port: 5432,
  auto_start: false,
};

const tab = (
  forwards: Record<number, { rule: ForwardRule; state: ForwardState }>,
) =>
  ({
    tabId: "t1",
    connectionId: "c1",
    sessionId: "s1",
    phase: "ready",
    forwards,
  }) as TabState;

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.forwardStart).mockResolvedValue(1);
  vi.mocked(api.forwardStop).mockResolvedValue();
  library.connections = [
    { id: "c1", name: "web", forwards: [local] } as unknown as Connection,
  ];
});

test("saved and active rules are listed once", () => {
  render(ForwardPanel, {
    tab: tab({
      3: { rule: local, state: { type: "Active", bound_port: 5432 } },
    }),
  });
  expect(screen.getAllByText("L 127.0.0.1:5432 → db:5432")).toHaveLength(1);
  expect(screen.getByText("Active on port 5432")).toBeTruthy();
});

test("saved rule starts with forwardStart", async () => {
  render(ForwardPanel, { tab: tab({}) });
  expect(screen.getByText("Stopped")).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: "Start" }));
  expect(api.forwardStart).toHaveBeenCalledWith("s1", local);
});

test("failed forward shows localized error and Start button", () => {
  render(ForwardPanel, {
    tab: tab({
      2: {
        rule: local,
        state: {
          type: "Failed",
          error: { code: "forward.port_in_use", params: { port: "5432" } },
        },
      },
    }),
  });
  expect(screen.getByText("Port 5432 is already in use.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Start" })).toBeTruthy();
});

test("stop calls forwardStop with id", async () => {
  render(ForwardPanel, {
    tab: tab({
      7: { rule: local, state: { type: "Active", bound_port: 5432 } },
    }),
  });
  await fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  expect(api.forwardStop).toHaveBeenCalledWith("s1", 7);
});

test("runtime tunnel is not persisted", async () => {
  render(ForwardPanel, { tab: tab({}) });
  await fireEvent.click(screen.getByRole("button", { name: "Add tunnel" }));
  await fireEvent.input(screen.getByLabelText("Bind port"), {
    target: { value: "8080" },
  });
  await fireEvent.input(screen.getByLabelText("Destination port"), {
    target: { value: "80" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Start tunnel" }));
  await waitFor(() =>
    expect(api.forwardStart).toHaveBeenCalledWith("s1", {
      type: "Local",
      bind_host: "127.0.0.1",
      bind_port: 8080,
      dest_host: "localhost",
      dest_port: 80,
      auto_start: false,
    }),
  );
  expect(api.connectionSave).not.toHaveBeenCalled();
});

test("remote tunnel has no bind address and invalid ports are rejected", async () => {
  render(ForwardPanel, { tab: tab({}) });
  await fireEvent.click(screen.getByRole("button", { name: "Add tunnel" }));
  await fireEvent.change(screen.getByLabelText("Type"), {
    target: { value: "Remote" },
  });
  expect(screen.queryByLabelText("Bind address")).toBeNull();
  await fireEvent.input(screen.getByLabelText("Bind port"), {
    target: { value: "0" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Start tunnel" }));
  expect(screen.getAllByText("Enter a port from 1 to 65535.")).toHaveLength(2);
  expect(api.forwardStart).not.toHaveBeenCalled();
});
