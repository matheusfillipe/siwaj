import type { Config } from "./generated/Config";

/** Carries the schema version, so a payload from an older shape is ignored
 * instead of being pushed at a device that no longer speaks it. */
const KEY = "siwaj.config.v2";

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
