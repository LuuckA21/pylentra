// Terminal palettes. The values come from the upstream projects:
//   Dracula  (c) Zeno Rocha, MIT License    https://github.com/dracula/dracula-theme
//   Nord     (c) Arctic Ice Studio, MIT     https://github.com/nordtheme/nord
//   Solarized (c) Ethan Schoonover, MIT     https://github.com/altercation/solarized
import type { ITheme } from "@xterm/xterm";
import type { TerminalTheme } from "./bindings/TerminalTheme";

// Colors come from the app's CSS tokens, so the terminal follows data-theme.
export function appTerminalTheme(): ITheme {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    background: v("--surface-1"),
    foreground: v("--fg"),
    cursor: v("--fg"),
    selectionBackground: v("--term-selection"),
  };
}

const solarized = {
  black: "#073642",
  red: "#dc322f",
  green: "#859900",
  yellow: "#b58900",
  blue: "#268bd2",
  magenta: "#d33682",
  cyan: "#2aa198",
  white: "#eee8d5",
  brightBlack: "#002b36",
  brightRed: "#cb4b16",
  brightGreen: "#586e75",
  brightYellow: "#657b83",
  brightBlue: "#839496",
  brightMagenta: "#6c71c4",
  brightCyan: "#93a1a1",
  brightWhite: "#fdf6e3",
};

export const PALETTES: Record<Exclude<TerminalTheme, "app">, ITheme> = {
  dracula: {
    background: "#282a36",
    foreground: "#f8f8f2",
    cursor: "#f8f8f2",
    selectionBackground: "#44475a",
    black: "#21222c",
    red: "#ff5555",
    green: "#50fa7b",
    yellow: "#f1fa8c",
    blue: "#bd93f9",
    magenta: "#ff79c6",
    cyan: "#8be9fd",
    white: "#f8f8f2",
    brightBlack: "#6272a4",
    brightRed: "#ff6e6e",
    brightGreen: "#69ff94",
    brightYellow: "#ffffa5",
    brightBlue: "#d6acff",
    brightMagenta: "#ff92df",
    brightCyan: "#a4ffff",
    brightWhite: "#ffffff",
  },
  nord: {
    background: "#2e3440",
    foreground: "#d8dee9",
    cursor: "#d8dee9",
    selectionBackground: "#434c5e",
    black: "#3b4252",
    red: "#bf616a",
    green: "#a3be8c",
    yellow: "#ebcb8b",
    blue: "#81a1c1",
    magenta: "#b48ead",
    cyan: "#88c0d0",
    white: "#e5e9f0",
    brightBlack: "#4c566a",
    brightRed: "#bf616a",
    brightGreen: "#a3be8c",
    brightYellow: "#ebcb8b",
    brightBlue: "#81a1c1",
    brightMagenta: "#b48ead",
    brightCyan: "#8fbcbb",
    brightWhite: "#eceff4",
  },
  solarized_dark: {
    ...solarized,
    background: "#002b36",
    foreground: "#839496",
    cursor: "#93a1a1",
    selectionBackground: "#073642",
  },
  solarized_light: {
    ...solarized,
    background: "#fdf6e3",
    foreground: "#657b83",
    cursor: "#586e75",
    selectionBackground: "#eee8d5",
  },
};

export const terminalTheme = (
  name: TerminalTheme,
  app: () => ITheme,
): ITheme => (name === "app" ? app() : PALETTES[name]);

// xterm's own ANSI colors, for the "app" card, whose theme sets none.
const XTERM_ANSI = ["#cd3131", "#0dbc79", "#e5e510", "#2472c8"];

/** Red, green, yellow and blue of a theme, for the previews. */
export const ansiChips = (th: ITheme): string[] => [
  th.red ?? XTERM_ANSI[0],
  th.green ?? XTERM_ANSI[1],
  th.yellow ?? XTERM_ANSI[2],
  th.blue ?? XTERM_ANSI[3],
];
