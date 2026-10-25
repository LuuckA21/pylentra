<script lang="ts" module>
  import type { ConnIcon } from "../../lib/bindings/ConnIcon";

  export type UiIcon =
    | "search"
    | "bolt"
    | "settings"
    | "plus"
    | "more"
    | "close"
    | "forward"
    | "shield"
    | "refresh"
    | "palette"
    | "chevron"
    | "terminal-ui"
    | "update"
    | "snippet"
    | "note";

  // 24x24 stroke paths drawn for this app.
  const PATHS: Record<ConnIcon | UiIcon, string[]> = {
    server: ["M4 4h16v6H4z", "M4 14h16v6H4z", "M8 7h.01M8 17h.01"],
    database: [
      "M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3z",
      "M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6",
      "M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
    ],
    web: [
      "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z",
      "M3 12h18",
      "M12 3c2.5 2.7 3.7 5.7 3.7 9s-1.2 6.3-3.7 9c-2.5-2.7-3.7-5.7-3.7-9S9.5 5.7 12 3z",
    ],
    nas: ["M3 7h18v10H3z", "M7 12h6", "M17 12h.01"],
    raspberry: [
      "M12 8c-2-3-5-3-6-1M12 8c2-3 5-3 6-1",
      "M12 8c-4 0-6 3-6 6s2.5 6 6 6 6-3 6-6-2-6-6-6z",
      "M10 13h.01M14 13h.01M12 17h.01",
    ],
    cloud: ["M7 18a4 4 0 0 1-.5-8A5.5 5.5 0 0 1 17 8.5 4.8 4.8 0 0 1 17 18z"],
    container: [
      "M12 3l8 4.5v9L12 21l-8-4.5v-9z",
      "M12 12v9",
      "M4 7.5l8 4.5 8-4.5",
    ],
    terminal: ["M3 5h18v14H3z", "M7 10l3 2-3 2", "M13 15h4"],
    search: ["M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14z", "M20 20l-4-4"],
    bolt: ["M13 3L5 13h6l-1 8 8-10h-6z"],
    snippet: [
      "M8 4c-2 0-3 1-3 3v2c0 1.5-1 2.5-2 3 1 .5 2 1.5 2 3v2c0 2 1 3 3 3",
      "M16 4c2 0 3 1 3 3v2c0 1.5 1 2.5 2 3-1 .5-2 1.5-2 3v2c0 2-1 3-3 3",
    ],
    // Sliders, as in the mockup: a sun-like gear read as the theme toggle.
    settings: [
      "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3",
      "M1 14h6M9 8h6M17 16h6",
    ],
    plus: ["M12 5v14M5 12h14"],
    more: ["M5 12h.01M12 12h.01M19 12h.01"],
    close: ["M6 6l12 12M18 6L6 18"],
    forward: ["M5 12h14", "M13 6l6 6-6 6"],
    shield: ["M12 3l8 3v6c0 4.5-3.4 8-8 9-4.6-1-8-4.5-8-9V6z"],
    refresh: [
      "M20 11a8 8 0 0 0-14.5-3.5L4 9",
      "M4 4v5h5",
      "M4 13a8 8 0 0 0 14.5 3.5L20 15",
      "M20 20v-5h-5",
    ],
    palette: [
      "M12 3a9 9 0 1 0 0 18c1.5 0 2-1 1.5-2-.5-1 0-2 1.5-2h2a4 4 0 0 0 4-4c0-5-4.5-10-9-10z",
      "M7.5 11h.01M10 7.5h.01M14.5 7.5h.01",
    ],
    chevron: ["M9 6l6 6-6 6"],
    "terminal-ui": ["M5 8l4 4-4 4", "M12 17h7"],
    note: ["M6 3h9l3 3v15H6z", "M9 10h6M9 14h6M9 18h4"],
    update: ["M12 4v12", "M7 11l5 5 5-5", "M5 20h14"],
  };
</script>

<script lang="ts">
  let { name, size = 18 }: { name: ConnIcon | UiIcon; size?: number } =
    $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#each PATHS[name] as d (d)}
    <path {d} />
  {/each}
</svg>
