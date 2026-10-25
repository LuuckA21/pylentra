import type { ApiError } from "../bindings/ApiError";
import { errorMessage } from "../i18n";

export type Toast = {
  id: number;
  kind: "error" | "warning" | "success";
  message: string;
};

export const toasts = $state<Toast[]>([]);

let next = 0;

export function dismiss(id: number): void {
  const i = toasts.findIndex((x) => x.id === id);
  if (i >= 0) toasts.splice(i, 1);
}

export function toast(message: string, kind: Toast["kind"] = "error"): void {
  const id = next++;
  toasts.push({ id, kind, message });
  setTimeout(() => dismiss(id), 8000);
}

export const isApiError = (e: unknown): e is ApiError =>
  typeof e === "object" && e !== null && "code" in e && "params" in e;

export function toastError(e: unknown): void {
  toast(isApiError(e) ? errorMessage(e) : String(e));
}
