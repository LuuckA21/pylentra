<script lang="ts">
  import type { TerminalTheme } from "../../lib/bindings/TerminalTheme";
  import type { CursorStyle } from "../../lib/bindings/CursorStyle";
  import { t } from "../../lib/i18n";
  import { radioKeydown } from "../../lib/radioKeys";
  import { settings } from "../../lib/stores/settings.svelte";
  import {
    ansiChips,
    appTerminalTheme,
    terminalTheme,
  } from "../../lib/terminalThemes";
  import NumberField from "./NumberField.svelte";
  import type { SectionProps } from "./props";
  import Toggle from "./Toggle.svelte";

  let { save, errors }: SectionProps = $props();
  const uid = $props.id();

  const THEMES: TerminalTheme[] = [
    "app",
    "dracula",
    "nord",
    "solarized_dark",
    "solarized_light",
  ];
  const CURSORS: CursorStyle[] = ["block", "bar", "underline"];

  // The app palette reads CSS tokens, so it is looked up again when the app theme changes.
  const palette = (name: TerminalTheme) => {
    void settings.theme;
    return terminalTheme(name, appTerminalTheme);
  };
  const current = $derived(palette(settings.terminal_theme));
  const chips = $derived(ansiChips(current));
</script>

<div class="field">
  <span id="{uid}-theme">{t("settings.terminalTheme")}</span>
  <div role="radiogroup" aria-labelledby="{uid}-theme" class="cards">
    {#each THEMES as name (name)}
      {@const th = palette(name)}
      <button
        type="button"
        role="radio"
        class="card"
        aria-checked={settings.terminal_theme === name}
        tabindex={settings.terminal_theme === name ? 0 : -1}
        onclick={() => save({ terminal_theme: name })}
        onkeydown={radioKeydown}
      >
        <span class="swatch" style:background={th.background}>
          {#each ansiChips(th) as c (c)}
            <i style:background={c}></i>
          {/each}
        </span>
        <span>{t(`settings.terminalTheme.${name}`)}</span>
      </button>
    {/each}
  </div>
</div>

<div class="pair">
  <div class="field">
    <label for="{uid}-family">{t("settings.fontFamily")}</label>
    <input
      id="{uid}-family"
      value={settings.font_family}
      onchange={(e) => {
        const v = e.currentTarget.value.trim();
        if (v) save({ font_family: v });
      }}
    />
  </div>
  <NumberField
    label={t("settings.fontSize")}
    value={settings.font_size}
    min={8}
    max={32}
    error={errors.font_size}
    save={(n) => save({ font_size: n })}
  />
</div>

<div class="pair">
  <div class="field">
    <span id="{uid}-cursor">{t("settings.cursor")}</span>
    <div class="segmented" role="radiogroup" aria-labelledby="{uid}-cursor">
      {#each CURSORS as c (c)}
        <label>
          <input
            type="radio"
            name="{uid}-cursor"
            value={c}
            checked={settings.cursor_style === c}
            onchange={() => save({ cursor_style: c })}
          />{t(`settings.cursor.${c}`)}
        </label>
      {/each}
    </div>
  </div>
  <Toggle
    label={t("settings.cursorBlink")}
    checked={settings.cursor_blink}
    onchange={(v) => save({ cursor_blink: v })}
  />
</div>

<NumberField
  label={t("settings.scrollback")}
  value={settings.scrollback}
  min={1000}
  max={100000}
  hint={t("settings.scrollbackHint")}
  error={errors.scrollback}
  save={(n) => save({ scrollback: n })}
/>

<Toggle
  label={t("settings.copyOnSelect")}
  hint={t("settings.copyOnSelectHint")}
  checked={settings.copy_on_select}
  onchange={(v) => save({ copy_on_select: v })}
/>

<div class="field">
  <span>{t("settings.preview")}</span>
  <!-- Static sample in the chosen palette; xterm itself only runs in tabs. -->
  <pre
    class="preview"
    aria-hidden="true"
    style:background={current.background}
    style:color={current.foreground}
    style:font-family={settings.font_family}
    style:font-size="{settings.font_size}px"><span style:color={chips[1]}
      >you@host</span
    >:<span style:color={chips[3]}>~</span>$ ls -l
drwxr-xr-x  <span style:color={chips[3]}>projects</span>
-rw-r--r--  notes.txt
<span style:color={chips[1]}>you@host</span>:<span style:color={chips[3]}
      >~</span
    >$ <span class="cursor" style:background={current.cursor}>&nbsp;</span
    ></pre>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
    align-items: end;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
    gap: var(--space-3);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-2);
    border: 2px solid var(--border);
    border-radius: var(--radius-md);
    text-align: left;
  }
  .card[aria-checked="true"] {
    border-color: var(--accent);
  }
  .swatch {
    display: flex;
    gap: var(--space-1);
    align-items: center;
    padding: var(--space-3) var(--space-2);
    border-radius: var(--radius-sm);
  }
  .swatch i {
    width: 14px;
    height: 14px;
    border-radius: 50%;
  }
  .preview {
    margin: 0;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
    line-height: 1.4;
  }
  .cursor {
    display: inline-block;
    width: 0.6em;
  }
</style>
