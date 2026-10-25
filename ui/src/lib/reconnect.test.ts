import { expect, test } from "vitest";
import type { CloseReason } from "./bindings/CloseReason";
import { shouldReconnect } from "./reconnect";

const err = (code: string): CloseReason => ({
  type: "Error",
  error: { code, params: {} },
});

test.each(["network.timeout", "network.refused", "ssh.protocol", "io"])(
  "%s reconnects",
  (code) => expect(shouldReconnect(err(code), true, true)).toBe(true),
);

test.each(["auth.all_failed", "hostkey.mismatch", "cancelled"])(
  "%s does not",
  (code) => expect(shouldReconnect(err(code), true, true)).toBe(false),
);

test("Exited and UserClosed do not", () => {
  expect(shouldReconnect({ type: "Exited", code: 0 }, true, true)).toBe(false);
  expect(shouldReconnect({ type: "UserClosed" }, true, true)).toBe(false);
});

test("never ready does not", () =>
  expect(shouldReconnect(err("network.timeout"), false, true)).toBe(false));

test("disabled does not", () =>
  expect(shouldReconnect(err("network.timeout"), true, false)).toBe(false));
