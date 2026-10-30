import { api } from "./api";
import type { ImportPick } from "./bindings/ImportPick";
import { t } from "./i18n";
import { isApiError, toast, toastError } from "./stores/toasts.svelte";

export async function exportLibrary(): Promise<void> {
  try {
    const done = await api.libraryExport(new Date().toISOString());
    if (!done) return;
    toast(t("library.exported", { path: done.path }), "success");
    if (!done.snippets) toast(t("library.exportedNoSnippets"), "warning");
  } catch (e) {
    toastError(e);
  }
}

/** The file the user picked and its preview, or null if cancelled or unreadable. */
export async function pickImport(): Promise<ImportPick | null> {
  try {
    return await api.libraryImportPreview();
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
  }
}
