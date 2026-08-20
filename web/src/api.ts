import type { ConfigState } from "./generated/ConfigState";
import type { Config } from "./generated/Config";
import type { ConfigSubmit } from "./generated/ConfigSubmit";
import type { DeviceStatus } from "./generated/DeviceStatus";

export type ServerState = ConfigState;

/// Null means the device stopped answering, which in config mode means it
/// went back to sleep rather than that anything failed.
export async function fetchStatus(): Promise<DeviceStatus | null> {
  try {
    const res = await fetch("/api/status");
    if (!res.ok) return null;
    return (await res.json()) as DeviceStatus;
  } catch {
    return null;
  }
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
  if (!res.ok) throw new Error(await failureMessage(res));
  return (await res.json()) as Config;
}

async function failureMessage(res: Response): Promise<string> {
  const text = await res.text();
  return text || `config save failed: ${res.status}`;
}
