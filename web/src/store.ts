import type { Config } from "./generated/Config";

const KEY = "siwaj.config.v1";

export interface ClientState {
  revision: number;
  config: Config;
}

export function loadClientState(): ClientState | null {
  const raw = localStorage.getItem(KEY);
  if (raw === null) return null;
  try {
    return JSON.parse(raw) as ClientState;
  } catch {
    return null;
  }
}

export function saveClientState(state: ClientState): void {
  localStorage.setItem(KEY, JSON.stringify(state));
}
