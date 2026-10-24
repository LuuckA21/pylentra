import { check, type Update } from "@tauri-apps/plugin-updater";

export const updates = $state<{ available: Update | null; checking: boolean }>({
  available: null,
  checking: false,
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
