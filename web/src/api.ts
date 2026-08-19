import type { Config } from "./generated/Config";
import type { ConfigSubmit } from "./generated/ConfigSubmit";

export interface ServerState {
  configured: boolean;
  revision: number;
  config: Config | null;
}

export async function fetchState(): Promise<ServerState> {
  const res = await fetch("/api/config");
  if (!res.ok) throw new Error(`config fetch failed: ${res.status}`);
  return (await res.json()) as ServerState;
}

export async function submitConfig(submit: ConfigSubmit): Promise<Config> {
  const res = await fetch("/api/config", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(submit),
  });
  if (!res.ok) throw new Error(`config save failed: ${res.status}`);
  return (await res.json()) as Config;
}
