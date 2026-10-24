import { warn } from "@tauri-apps/plugin-log";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export const updates = $state<{
  available: Update | null;
  checking: boolean;
  installing: boolean;
}>({
  available: null,
  checking: false,
  installing: false,
});

// Rejects on failure: the startup check only logs it, "Check now" shows it.
export async function checkForUpdate(): Promise<Update | null> {
  updates.checking = true;
  try {
    return (updates.available = await check());
  } finally {
    updates.checking = false;
  }
}

// Shared by the app banner and the Updates settings section.
export async function installUpdate(): Promise<void> {
  if (!updates.available) return;
  updates.installing = true;
  try {
    await updates.available.downloadAndInstall();
    await relaunch();
  } catch (e) {
    warn(`update install failed: ${e}`);
    updates.installing = false;
  }
}
