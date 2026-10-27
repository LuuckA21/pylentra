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

const onclose = () => {};

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
    open: true,
    onclose,
    tab: tab({
      3: { rule: local, state: { type: "Active", bound_port: 5432 } },
    }),
  });
  expect(screen.getAllByText("L 127.0.0.1:5432 → db:5432")).toHaveLength(1);
  expect(screen.getByText("Active on port 5432")).toBeTruthy();
});

test("saved rule starts with forwardStart", async () => {
  render(ForwardPanel, { tab: tab({}), open: true, onclose });
  expect(screen.getByText("Stopped")).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: "Start" }));
  expect(api.forwardStart).toHaveBeenCalledWith("s1", local);
});

test("failed forward shows localized error and Start button", () => {
  render(ForwardPanel, {
    open: true,
    onclose,
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

test("panel opens when a forward fails", async () => {
  const { rerender } = render(ForwardPanel, {
    tab: tab({}),
    open: false,
    onclose,
  });
  expect(screen.queryByRole("dialog", { name: "Port forwarding" })).toBeNull();
  await rerender({
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
  expect(screen.getByRole("dialog", { name: "Port forwarding" })).toBeTruthy();
});

test("close button and Escape call onclose", async () => {
  const close = vi.fn();
  render(ForwardPanel, { tab: tab({}), open: true, onclose: close });
  await fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(close).toHaveBeenCalledTimes(2);
});

test("stop calls forwardStop with id", async () => {
  render(ForwardPanel, {
    open: true,
    onclose,
    tab: tab({
      7: { rule: local, state: { type: "Active", bound_port: 5432 } },
    }),
  });
  await fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  expect(api.forwardStop).toHaveBeenCalledWith("s1", 7);
});

test("runtime tunnel is not persisted", async () => {
  render(ForwardPanel, { tab: tab({}), open: true, onclose });
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
  render(ForwardPanel, { tab: tab({}), open: true, onclose });
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

const failedTab = () =>
  tab({
    1: {
      rule: local,
      state: {
        type: "Failed",
        error: { code: "forward.port_in_use", params: { port: "5432" } },
      },
    },
  });

test("opening moves focus into the panel and closing returns it", async () => {
  const opener = document.createElement("button");
  document.body.append(opener);
  opener.focus();
  const close = vi.fn();
  const { rerender } = render(ForwardPanel, {
    tab: tab({}),
    open: false,
    onclose: close,
  });
  await rerender({ open: true });
  await waitFor(() =>
    expect(document.activeElement).toBe(screen.getByRole("dialog")),
  );
  await fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(close).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(opener);
  opener.remove();
});

test("a panel opened by a failed forward does not take focus", async () => {
  const input = document.createElement("input");
  document.body.append(input);
  input.focus();
  render(ForwardPanel, { tab: failedTab(), onclose });
  await screen.findByRole("dialog");
  expect(document.activeElement).toBe(input);
  input.remove();
});

test("a forward that stays failed does not reopen a closed panel", async () => {
  const { rerender } = render(ForwardPanel, { tab: failedTab(), onclose });
  await screen.findByRole("dialog");
  // What the parent does on close, then a later tab update with the same failure.
  await rerender({ open: false });
  await rerender({ tab: failedTab() });
  expect(screen.queryByRole("dialog")).toBeNull();
});

const socks: ForwardRule = {
  type: "Dynamic",
  bind_host: "127.0.0.1",
  bind_port: 1080,
  auto_start: false,
};

test("SOCKS quick form asks only for the listening address and port", async () => {
  render(ForwardPanel, { tab: tab({}), open: true, onclose });
  await fireEvent.click(screen.getByRole("button", { name: "Add tunnel" }));
  await fireEvent.change(screen.getByLabelText("Type"), {
    target: { value: "Dynamic" },
  });
  expect(screen.queryByLabelText("Destination host")).toBeNull();
  expect(screen.queryByLabelText("Destination port")).toBeNull();
  expect(
    (screen.getByLabelText("Bind address") as HTMLInputElement).value,
  ).toBe("127.0.0.1");
  expect((screen.getByLabelText("Bind port") as HTMLInputElement).value).toBe(
    "1080",
  );
  await fireEvent.click(screen.getByRole("button", { name: "Start tunnel" }));
  await waitFor(() =>
    expect(api.forwardStart).toHaveBeenCalledWith("s1", socks),
  );
});

test("an active SOCKS rule shows the proxy hint with the real port", () => {
  library.connections = [
    { id: "c1", name: "web", forwards: [socks] } as unknown as Connection,
  ];
  render(ForwardPanel, {
    open: true,
    onclose,
    tab: tab({
      1: { rule: socks, state: { type: "Active", bound_port: 40000 } },
    }),
  });
  expect(screen.getByText("SOCKS 127.0.0.1:1080")).toBeTruthy();
  expect(
    screen.getByText(/Use 127\.0\.0\.1:40000 as a SOCKS5 proxy/),
  ).toBeTruthy();
});

test.each([
  ["::1", "[::1]:40000"],
  ["0.0.0.0", "127.0.0.1:40000"],
  ["::", "127.0.0.1:40000"],
  ["192.168.1.5", "192.168.1.5:40000"],
])("the SOCKS hint for a rule bound to %s uses %s", (bind_host, shown) => {
  const rule: ForwardRule = { ...socks, bind_host };
  library.connections = [
    { id: "c1", name: "web", forwards: [rule] } as unknown as Connection,
  ];
  render(ForwardPanel, {
    open: true,
    onclose,
    tab: tab({
      1: { rule, state: { type: "Active", bound_port: 40000 } },
    }),
  });
  expect(
    screen.getByText(
      new RegExp(`Use ${shown.replace(/[[\]]/g, "\\$&")} as a SOCKS5`),
    ),
  ).toBeTruthy();
});
