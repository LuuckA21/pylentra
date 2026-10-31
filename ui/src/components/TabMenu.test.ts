import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    sessionOpen: vi.fn().mockResolvedValue("s"),
    sessionClose: vi.fn().mockResolvedValue(undefined),
  },
}));

import { setLang } from "../lib/i18n";
import { rects } from "../lib/layout";
import {
  openTab,
  sessions,
  splitPane,
  tabOf,
  type Tab,
} from "../lib/stores/sessions.svelte";
import TabMenu from "./TabMenu.svelte";

beforeEach(() => {
  sessions.tabs.splice(0);
  sessions.panes.splice(0);
  sessions.active = null;
  setLang("en");
});

const names: Record<string, string> = {};
const tabFor = (paneId: string): Tab => tabOf(paneId)!;
const open = (tab: Tab) => {
  const onclose = vi.fn();
  render(TabMenu, {
    tab,
    x: 10,
    y: 10,
    titleOf: (paneId: string) => names[paneId],
    onclose,
  });
  return onclose;
};
const item = (name: string) => screen.getByRole("menuitem", { name });

test("merges the tab into another one", async () => {
  const a = openTab("c1");
  const b = openTab("c2");
  Object.assign(names, { [a]: "web-1", [b]: "web-2" });
  const onclose = open(tabFor(b));
  expect(document.activeElement).toBe(item("Merge into web-1"));
  await fireEvent.click(item("Merge into web-1"));
  expect(sessions.tabs).toHaveLength(1);
  expect(Object.keys(rects(sessions.tabs[0].root).panes)).toEqual([a, b]);
  expect(onclose).toHaveBeenCalledWith(false);
});

test("a merge past 4 panes is disabled with the reason", async () => {
  const a = openTab("c1");
  splitPane(a, "row", "c2");
  splitPane(a, "row", "c3");
  const b = openTab("c4");
  splitPane(b, "row", "c5");
  // A tab is named after its focused pane, here the last one split off.
  names[tabFor(a).focus] = "web-1";
  open(tabFor(b));
  const merge = item("Merge into web-1");
  expect(merge).toHaveProperty("disabled", true);
  expect(merge.getAttribute("title")).toBe("More than 4 panes: no room");
  await fireEvent.click(merge);
  expect(sessions.tabs).toHaveLength(2);
});

test("moves the focused pane of a split tab to a new tab", async () => {
  const a = openTab("c1");
  const b = splitPane(a, "row", "c2")!;
  names[b] = "db";
  const onclose = open(tabFor(a));
  await fireEvent.click(item("Move db to a new tab"));
  expect(sessions.tabs).toHaveLength(2);
  expect(tabOf(b)!.root).toEqual({ pane: b });
  expect(onclose).toHaveBeenCalledWith(false);
});

test("a lone tab says there is nothing to do", () => {
  const a = openTab("c1");
  open(tabFor(a));
  expect(item("No other tabs")).toHaveProperty("disabled", true);
});

test("arrows move between items and Escape closes, refocusing the tab", async () => {
  const a = openTab("c1");
  const b = splitPane(a, "row", "c2")!;
  const c = openTab("c3");
  Object.assign(names, { [b]: "db", [c]: "web-3" });
  const onclose = open(tabFor(a));
  const menu = screen.getByRole("menu");
  expect(document.activeElement).toBe(item("Merge into web-3"));
  await fireEvent.keyDown(menu, { key: "ArrowDown" });
  expect(document.activeElement).toBe(item("Move db to a new tab"));
  await fireEvent.keyDown(menu, { key: "ArrowDown" });
  expect(document.activeElement).toBe(item("Merge into web-3"));
  await fireEvent.keyDown(menu, { key: "Escape" });
  expect(onclose).toHaveBeenCalledWith(true);
});
