import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Connection } from "../lib/bindings/Connection";
import { recordRecent } from "../lib/recent";
import { library } from "../lib/stores/library.svelte";
import Welcome from "./Welcome.svelte";

const conn = (id: string, name: string) =>
  ({
    id,
    name,
    group_id: null,
    host: `${name}.example.com`,
    port: 22,
    username: "luca",
  }) as Connection;

beforeEach(() => {
  localStorage.clear();
  library.groups = [];
  library.connections = [conn("a", "web"), conn("b", "db"), conn("c", "nas")];
});

test("lists recent connections, most recent first, skipping deleted ones", async () => {
  for (const id of ["a", "gone", "b", "c"]) recordRecent(id);
  const onopen = vi.fn();
  render(Welcome, { onopen });
  const names = screen
    .getAllByRole("button")
    .map((b) => b.textContent?.replace(/\s+/g, " ").trim());
  expect(names).toEqual([
    expect.stringContaining("nas"),
    expect.stringContaining("db"),
    expect.stringContaining("web"),
  ]);
  await fireEvent.click(screen.getAllByRole("button")[1]);
  expect(onopen).toHaveBeenCalledWith("b");
});

test("without recents shows only the welcome text", () => {
  render(Welcome, { onopen: () => {} });
  expect(screen.getByText("Open a connection from the list.")).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.queryByText("Recent connections")).toBeNull();
});
