import { expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Connection } from "../lib/bindings/Connection";
import type { Group } from "../lib/bindings/Group";
import type { TabState } from "../lib/sessionReducer";
import StatusBar from "./StatusBar.svelte";

const groups: Group[] = [{ id: "g1", name: "Production", color: "red" }];
const connection = { id: "c1", name: "web", group_id: "g1" } as Connection;
const rule = {
  type: "Local",
  bind_host: "127.0.0.1",
  bind_port: 5432,
  dest_host: "localhost",
  dest_port: 5432,
  auto_start: false,
} as const;

const tab = (readyAt: number): TabState => ({
  tabId: "t1",
  connectionId: "c1",
  sessionId: "s1",
  phase: "ready",
  readyAt,
  forwards: {
    1: { rule, state: { type: "Active", bound_port: 5432 } },
    2: { rule: { ...rule, bind_port: 9 }, state: { type: "Stopped" } },
  },
});

test("shows the group pill, one pill per active forward and the connected time", async () => {
  const onforwards = vi.fn();
  render(StatusBar, {
    tab: tab(Date.now() - 2 * 3600_000),
    connection,
    groups,
    onforwards,
  });
  expect(screen.getByText("Production")).toBeTruthy();
  expect(screen.getAllByText(/⇄/)).toHaveLength(1);
  await fireEvent.click(screen.getByText("⇄ 5432 → localhost:5432"));
  expect(onforwards).toHaveBeenCalledTimes(1);
  expect(screen.getByText("2 h")).toBeTruthy();
});

test("an active SOCKS forward shows a SOCKS pill", () => {
  const socks = {
    type: "Dynamic",
    bind_host: "127.0.0.1",
    bind_port: 1080,
    auto_start: false,
  } as const;
  render(StatusBar, {
    tab: {
      ...tab(Date.now()),
      forwards: {
        1: { rule: socks, state: { type: "Active", bound_port: 1080 } },
      },
    },
    connection,
    groups,
    onforwards: () => {},
  });
  expect(screen.getByText(/SOCKS :1080/)).toBeTruthy();
});

test("short sessions read '< 1 min' and minutes count up", () => {
  const { unmount } = render(StatusBar, {
    tab: tab(Date.now()),
    connection,
    groups,
    onforwards: () => {},
  });
  expect(screen.getByText("< 1 min")).toBeTruthy();
  unmount();
  render(StatusBar, {
    tab: tab(Date.now() - 5 * 60_000),
    connection,
    groups,
    onforwards: () => {},
  });
  expect(screen.getByText("5 min")).toBeTruthy();
});

test("the Forward button and pills report whether the panel is open", () => {
  render(StatusBar, {
    tab: tab(Date.now()),
    connection,
    groups,
    onforwards: vi.fn(),
    forwardsOpen: true,
  });
  expect(
    screen
      .getByRole("button", { name: "Forward" })
      .getAttribute("aria-expanded"),
  ).toBe("true");
  expect(
    screen
      .getByRole("button", { name: "⇄ 5432 → localhost:5432" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
});

test("the snippets button is disabled outside ready and opens the palette", async () => {
  const onsnippets = vi.fn();
  const base = tab(Date.now());
  const { rerender } = render(StatusBar, {
    tab: { ...base, phase: "connecting" },
    connection,
    groups,
    onforwards: vi.fn(),
    onsnippets,
  });
  const button = screen.getByRole("button", { name: "Snippets" });
  expect(button).toHaveProperty("disabled", true);
  await rerender({ tab: base, snippetsOpen: true });
  expect(button).toHaveProperty("disabled", false);
  expect(button.getAttribute("aria-expanded")).toBe("true");
  await fireEvent.click(button);
  expect(onsnippets).toHaveBeenCalledTimes(1);
});
