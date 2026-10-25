import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: { snippetsGet: vi.fn(), snippetSave: vi.fn() },
}));

import { api } from "../lib/api";
import type { Connection } from "../lib/bindings/Connection";
import { library } from "../lib/stores/library.svelte";
import SnippetForm from "./SnippetForm.svelte";

const conn = (id: string, name: string): Connection => ({
  id,
  name,
  group_id: null,
  host: "h",
  port: 22,
  username: "u",
  auth: { type: "Agent" },
  jump_via: null,
  forwards: [],
});

function setup(props: Record<string, unknown> = {}) {
  const p = { onclose: vi.fn(), onsaved: vi.fn(), ...props };
  render(SnippetForm, p);
  return p;
}
const type = (label: string, value: string) =>
  fireEvent.input(screen.getByLabelText(label), { target: { value } });
const save = () =>
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
const saved = () => vi.mocked(api.snippetSave).mock.calls.at(-1)?.[0];

beforeEach(() => {
  vi.resetAllMocks();
  library.connections = [conn("a", "Alpha"), conn("b", "Beta")];
  vi.mocked(api.snippetSave).mockResolvedValue();
  vi.mocked(api.snippetsGet).mockResolvedValue([]);
});

test("saves name, text, run and scope", async () => {
  const { onsaved, onclose } = setup();
  await type("Name", " Log ");
  await type("Text", "tail -f x\n");
  await fireEvent.click(
    screen.getByRole("switch", { name: "Run immediately" }),
  );
  await fireEvent.change(screen.getByLabelText("Available in"), {
    target: { value: "a" },
  });
  await save();
  await waitFor(() => expect(onsaved).toHaveBeenCalled());
  expect(onclose).toHaveBeenCalled();
  expect(saved()).toEqual({
    id: expect.any(String),
    name: "Log",
    text: "tail -f x",
    run: true,
    connection_id: "a",
  });
});

test("global omits connection_id", async () => {
  setup();
  await type("Name", "Log");
  await type("Text", "ls");
  await save();
  await waitFor(() => expect(api.snippetSave).toHaveBeenCalled());
  expect(saved()).not.toHaveProperty("connection_id");
});

test("preset fills text and scope", () => {
  setup({ preset: { text: "ls", connectionId: "a" } });
  expect((screen.getByLabelText("Text") as HTMLTextAreaElement).value).toBe(
    "ls",
  );
  expect(
    (screen.getByLabelText("Available in") as HTMLSelectElement).value,
  ).toBe("a");
});

test("empty fields block saving", async () => {
  setup();
  await save();
  const name = screen.getByLabelText("Name");
  expect(name.getAttribute("aria-invalid")).toBe("true");
  expect(
    document.getElementById(name.getAttribute("aria-describedby")!)
      ?.textContent,
  ).toBe("Required.");
  expect(screen.getByLabelText("Text").getAttribute("aria-invalid")).toBe(
    "true",
  );
  expect(api.snippetSave).not.toHaveBeenCalled();
});

test("core error shown at field", async () => {
  vi.mocked(api.snippetSave).mockRejectedValue({
    code: "config.invalid",
    params: { field: "text", reason: "too long" },
  });
  const { onsaved } = setup();
  await type("Name", "Log");
  await type("Text", "ls");
  await save();
  const text = screen.getByLabelText("Text");
  await waitFor(() => expect(text.getAttribute("aria-invalid")).toBe("true"));
  expect(
    document.getElementById(text.getAttribute("aria-describedby")!)
      ?.textContent,
  ).toContain("too long");
  expect(onsaved).not.toHaveBeenCalled();
});

test("editing keeps the id and prefills", async () => {
  setup({
    initial: { id: "x", name: "N", text: "t", run: true, connection_id: "b" },
  });
  await save();
  await waitFor(() => expect(api.snippetSave).toHaveBeenCalled());
  expect(saved()).toEqual({
    id: "x",
    name: "N",
    text: "t",
    run: true,
    connection_id: "b",
  });
});
