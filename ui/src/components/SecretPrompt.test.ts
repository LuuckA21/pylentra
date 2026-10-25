import { expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Connection } from "../lib/bindings/Connection";
import { library } from "../lib/stores/library.svelte";
import SecretPrompt from "./SecretPrompt.svelte";

const conn = (o: Partial<Connection>): Connection => ({
  id: "c1",
  name: "web",
  group_id: null,
  host: "web",
  port: 22,
  username: "luca",
  auth: { type: "Password", save: true },
  jump_via: null,
  forwards: [],
  ...o,
});

test("password prompt names the hop and defaults save to its preference", async () => {
  library.connections = [
    conn({ jump_via: "c2" }),
    conn({
      id: "c2",
      // Saved with capitals; the core lowercases the prompt's host.
      host: "Bastion",
      port: 2222,
      username: "jump",
      auth: { type: "Key", path: "k", save_passphrase: false },
    }),
  ];
  const onanswer = vi.fn();
  render(SecretPrompt, {
    prompt: {
      type: "SecretPrompt",
      host: "bastion",
      port: 2222,
      kind: { type: "Passphrase", attempt: 2 },
      can_save: true,
    },
    connectionId: "c1",
    onanswer,
  });
  expect(screen.getByRole("heading").textContent).toContain(
    "jump@bastion:2222",
  );
  expect(screen.getByText("Attempt 2 of 3")).toBeTruthy();
  const save = screen.getByLabelText("Save to keychain") as HTMLInputElement;
  expect(save.checked).toBe(false);
  await fireEvent.input(screen.getByLabelText("Passphrase"), {
    target: { value: "pp" },
  });
  await fireEvent.click(screen.getByText("Connect"));
  expect(onanswer).toHaveBeenCalledWith({
    type: "Secret",
    value: "pp",
    save: false,
  });
});

test("keyboard-interactive shows one field per prompt and Esc cancels", async () => {
  library.connections = [conn({})];
  const onanswer = vi.fn();
  render(SecretPrompt, {
    prompt: {
      type: "SecretPrompt",
      host: "web",
      port: 22,
      kind: {
        type: "KeyboardInteractive",
        name: "",
        instructions: "",
        prompts: [
          { text: "Code:", echo: true },
          { text: "PIN:", echo: false },
        ],
      },
      can_save: true,
    },
    connectionId: "c1",
    onanswer,
  });
  expect((screen.getByLabelText("Code:") as HTMLInputElement).type).toBe(
    "text",
  );
  expect((screen.getByLabelText("PIN:") as HTMLInputElement).type).toBe(
    "password",
  );
  // The core never saves keyboard-interactive answers.
  expect(screen.queryByLabelText("Save to keychain")).toBeNull();
  await fireEvent.keyDown(screen.getByLabelText("PIN:"), { key: "Escape" });
  expect(onanswer).toHaveBeenCalledWith({ type: "Cancel" });
});

test("a prompt that cannot save hides the option and answers save false", async () => {
  library.connections = [conn({ auth: { type: "Auto" } })];
  const onanswer = vi.fn();
  render(SecretPrompt, {
    prompt: {
      type: "SecretPrompt",
      host: "web",
      port: 22,
      kind: { type: "Password" },
      can_save: false,
    },
    connectionId: "c1",
    onanswer,
  });
  expect(screen.queryByLabelText("Save to keychain")).toBeNull();
  await fireEvent.input(screen.getByLabelText("Password"), {
    target: { value: "pw" },
  });
  await fireEvent.click(screen.getByText("Connect"));
  expect(onanswer).toHaveBeenCalledWith({
    type: "Secret",
    value: "pw",
    save: false,
  });
});

test("a quick tab's prompt names its user", () => {
  library.connections = [];
  render(SecretPrompt, {
    prompt: {
      type: "SecretPrompt",
      host: "10.0.0.5",
      port: 2200,
      kind: { type: "Password" },
      can_save: false,
    },
    connectionId: "q1",
    quick: conn({
      id: "q1",
      host: "10.0.0.5",
      port: 2200,
      username: "root",
      auth: { type: "Auto" },
    }),
    onanswer: vi.fn(),
  });
  expect(screen.getByRole("heading").textContent).toContain(
    "root@10.0.0.5:2200",
  );
});
