import { api } from "../api";
import type { Snippet } from "../bindings/Snippet";

export const snippets = $state<{ list: Snippet[] }>({ list: [] });

export async function loadSnippets(): Promise<void> {
  snippets.list = await api.snippetsGet();
}

// Like the library: the Rust side stays the source of truth, so reload after each write.
export async function saveSnippet(s: Snippet): Promise<void> {
  await api.snippetSave(s);
  await loadSnippets();
}

export async function deleteSnippet(id: string): Promise<void> {
  await api.snippetDelete(id);
  await loadSnippets();
}
