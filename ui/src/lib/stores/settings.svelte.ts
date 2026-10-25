import { api } from "../api";
import type { Settings } from "../bindings/Settings";
import { detectLang, setLang } from "../i18n";

export const settings = $state<Settings>({
  theme: "System",
  font_family: "monospace",
  font_size: 14,
  language: null,
  terminal_theme: "app",
  cursor_style: "block",
  cursor_blink: true,
  scrollback: 10000,
  copy_on_select: false,
  default_user: null,
  default_port: 22,
  connect_timeout_secs: 15,
  keepalive_secs: 30,
  auto_reconnect: false,
  confirm_close_active: true,
  check_updates: true,
});

// "system" | "light" | "dark": the CSS decides what "system" looks like.
function apply(): void {
  document.documentElement.dataset.theme = settings.theme.toLowerCase();
  setLang(
    settings.language === "it" || settings.language === "en"
      ? settings.language
      : detectLang(navigator.language),
  );
}

export async function loadSettings(): Promise<void> {
  Object.assign(settings, await api.settingsGet());
  apply();
}

export async function saveSettings(patch: Partial<Settings>): Promise<void> {
  const next = { ...$state.snapshot(settings), ...patch };
  await api.settingsSave(next);
  Object.assign(settings, next);
  apply();
}

export const quickDefaults = (): { user: string | null; port: number } => ({
  user: settings.default_user,
  port: settings.default_port,
});
