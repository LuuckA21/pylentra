import type { Settings } from "../../lib/bindings/Settings";

/** What the container hands every section: the saver and the core's per-field errors. */
export type SectionProps = {
  save: (patch: Partial<Settings>) => void;
  errors: Record<string, string>;
};
