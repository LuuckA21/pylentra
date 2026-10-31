import { api } from "./api";
import type { ImportRow } from "./bindings/ImportRow";
import type { ImportPick } from "./bindings/ImportPick";
import { t } from "./i18n";
import { current } from "./i18n/lang.svelte";
import { isApiError, toast, toastError } from "./stores/toasts.svelte";

// A file dialog is open: a second click must not open another.
let busy = false;

/** Local time with its offset, e.g. 2026-10-30T00:30:00+01:00: the file name takes its date. */
function localTime(d = new Date()): string {
  const off = -d.getTimezoneOffset();
  const pad = (n: number) => String(n).padStart(2, "0");
  const local = new Date(d.getTime() + off * 60_000).toISOString().slice(0, 19);
  const abs = Math.abs(off);
  return `${local}${off < 0 ? "-" : "+"}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`;
}

export async function exportLibrary(): Promise<void> {
  if (busy) return;
  busy = true;
  try {
    const done = await api.libraryExport(localTime());
    if (!done) return;
    toast(t("library.exported", { path: done.path }), "success");
    if (!done.snippets) toast(t("library.exportedNoSnippets"), "warning");
  } catch (e) {
    toastError(e);
  } finally {
    busy = false;
  }
}

/** The file the user picked and its preview, or null if cancelled, unreadable or already picking. */
export async function pickImport(): Promise<ImportPick | null> {
  if (busy) return null;
  busy = true;
  try {
    return await api.libraryImportPreview(current.lang);
  } catch (e) {
    if (isApiError(e) && e.code === "config.unsupported_version")
      toast(t("library.newerVersion"));
    else if (
      isApiError(e) &&
      e.code === "config.invalid" &&
      e.params.field === "format"
    )
      toast(t("library.notExport"));
    else toastError(e);
    return null;
  } finally {
    busy = false;
  }
}

/** The jump host note of a row, worded from what is selected; null without one. */
export function jumpText(row: ImportRow, selected: Set<string>): string | null {
  const j = row.jump;
  if (!j) return null;
  if (j.in_file && selected.has(j.in_file))
    return t("library.via", { name: j.name });
  return t(j.existing ? "library.viaExisting" : "library.jumpDropped", {
    name: j.name,
  });
}
