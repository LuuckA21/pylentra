import type { Lang } from "./index";

// Rune state lives here (not in index.ts) so t() re-renders components on language change.
export const current = $state<{ lang: Lang }>({ lang: "en" });
