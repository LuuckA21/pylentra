import { expect, test } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { t } from "../lib/i18n";
import HostKeyDialog from "./HostKeyDialog.svelte";

const prompt = (cert_authority: boolean) =>
  ({
    type: "HostKeyPrompt",
    host: "h",
    port: 22,
    algo: "ssh-ed25519",
    fingerprint: "SHA256:x",
    cert_authority,
  }) as const;

test("a cert-authority warning is shown and described by the dialog", () => {
  render(HostKeyDialog, { prompt: prompt(true), onanswer: () => {} });
  const warning = screen.getByText(t("hostkey.certAuthority"));
  expect(screen.getByRole("dialog").getAttribute("aria-describedby")).toContain(
    warning.id,
  );
});

test("no warning without a cert-authority line", () => {
  render(HostKeyDialog, { prompt: prompt(false), onanswer: () => {} });
  expect(screen.queryByText(t("hostkey.certAuthority"))).toBeNull();
});
