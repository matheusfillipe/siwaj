import type { ConfigState } from "./generated/ConfigState";
import type { Config } from "./generated/Config";
import type { ConfigSubmit } from "./generated/ConfigSubmit";

export type ServerState = ConfigState;

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
  if (!res.ok) throw new Error(await failureMessage(res));
  return (await res.json()) as Config;
}

async function failureMessage(res: Response): Promise<string> {
  const text = await res.text();
  return text || `config save failed: ${res.status}`;
}
