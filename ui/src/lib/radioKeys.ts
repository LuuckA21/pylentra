/** Arrow keys on a role="radio" button move focus and selection to the neighbour, wrapping around. */
export function radioKeydown(e: KeyboardEvent) {
  const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[
    e.key
  ];
  if (!step) return;
  e.preventDefault();
  const radios = Array.from(
    (
      e.currentTarget as HTMLElement
    ).parentElement!.querySelectorAll<HTMLElement>("[role=radio]"),
  );
  const i = radios.indexOf(e.currentTarget as HTMLElement);
  const next = radios[(i + step + radios.length) % radios.length];
  next.focus();
  next.click();
}
