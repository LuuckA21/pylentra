import { expect, test } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { t } from "../lib/i18n";
import ErrorOverlay from "./ErrorOverlay.svelte";

const error = { code: "hostkey.revoked", params: { host: "h", port: "22" } };

test.each([
  ["Pylentra", "session.revokedPylentra"],
  ["OpenSsh", "session.revokedOpenSsh"],
  ["System", "session.revokedSystem"],
] as const)(
  "revoked key from %s shows fingerprint and hint only",
  (source, key) => {
    render(ErrorOverlay, {
      error,
      mismatch: undefined,
      revoked: {
        type: "HostKeyRevoked",
        host: "h",
        port: 22,
        fingerprint: "SHA256:bad",
        source,
      },
      onretry: () => {},
      onedit: () => {},
    });
    expect(screen.getByText(t("session.revoked"))).toBeTruthy();
    expect(screen.getByText("SHA256:bad")).toBeTruthy();
    expect(screen.getByText(t(key))).toBeTruthy();
    expect(screen.queryByText(t("session.forget"))).toBeNull();
    expect(screen.queryByText(/ssh-keygen -R/)).toBeNull();
  },
);
