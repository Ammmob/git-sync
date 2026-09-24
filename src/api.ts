import { invoke } from "@tauri-apps/api/core";
import type { Snapshot } from "./types";
const preview = import.meta.env.VITE_UI_PREVIEW === "true";
let adapter: typeof import("./preview") | undefined;
async function demo() {
  return (adapter ??= await import("./preview"));
}
export async function snapshot(): Promise<Snapshot> {
  return preview ? (await demo()).snapshot() : invoke("snapshot");
}
export async function request<T = unknown>(
  operation: string,
  payload: unknown = {},
): Promise<T> {
  return preview
    ? ((await demo()).request(operation, payload) as T)
    : invoke("request", { operation, payload });
}
export { preview };
