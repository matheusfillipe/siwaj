import { fetchState, fetchStatus, submitConfig } from "./api";
import { BandPicker } from "./bands";
import { loadClientState, saveClientState } from "./store";
import type { Config } from "./generated/Config";
import type { ConfigSubmit } from "./generated/ConfigSubmit";

interface Elements {
  awake: HTMLElement;
  banner: HTMLElement;
  status: HTMLElement;
  form: HTMLFormElement;
  city: HTMLInputElement;
  located: HTMLElement;
  rain: HTMLInputElement;
  refresh: HTMLSelectElement;
  save: HTMLButtonElement;
  rainOut: HTMLOutputElement;
  bands: HTMLElement;
  bandJacket: HTMLElement;
  bandPullover: HTMLElement;
  bandShirt: HTMLElement;
  handleLow: HTMLElement;
  handleHigh: HTMLElement;
}

function elements(): Elements {
  return {
    awake: document.getElementById("awake") as HTMLElement,
    banner: document.getElementById("banner") as HTMLElement,
    status: document.getElementById("status") as HTMLElement,
    form: document.getElementById("config-form") as HTMLFormElement,
    city: document.getElementById("city") as HTMLInputElement,
    located: document.getElementById("located") as HTMLElement,
    rain: document.getElementById("rain") as HTMLInputElement,
    refresh: document.getElementById("refresh") as HTMLSelectElement,
    save: document.getElementById("save") as HTMLButtonElement,
    rainOut: document.getElementById("rainOut") as HTMLOutputElement,
    bands: document.getElementById("bands") as HTMLElement,
    bandJacket: document.getElementById("bandJacket") as HTMLElement,
    bandPullover: document.getElementById("bandPullover") as HTMLElement,
    bandShirt: document.getElementById("bandShirt") as HTMLElement,
    handleLow: document.getElementById("handleLow") as HTMLElement,
    handleHigh: document.getElementById("handleHigh") as HTMLElement,
  };
}

function clock(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  return `${mins}:${String(seconds % 60).padStart(2, "0")}`;
}

/**
 * Editing sends nothing on its own, so a long setup would run the device's
 * idle window down and lose the save. Reading the config counts as a request,
 * which is enough to hold the window open while someone is working.
 */
const KEEPALIVE_MS = 60_000;
let lastKeepAlive = 0;

function keepAwake(): void {
  const now = Date.now();
  if (now - lastKeepAlive < KEEPALIVE_MS) return;
  lastKeepAlive = now;
  void fetchState().catch(() => undefined);
}

/**
 * Losing the device looks the same from here whether it slept, dropped off the
 * network, or died, so the page reports only what it knows.
 */
function watchAwake(el: Elements): void {
  const tick = async (): Promise<void> => {
    const status = await fetchStatus();
    if (status === null) {
      el.awake.textContent = "Unreachable. If it went to sleep, hold the button.";
      el.awake.classList.add("unreachable");
      return;
    }
    el.awake.textContent = `Awake for ${clock(status.secondsUntilSleep)}`;
    el.awake.classList.remove("unreachable");
  };
  void tick();
  window.setInterval(() => void tick(), 5000);
}

function setStatus(el: Elements, message: string, isError = false): void {
  el.status.textContent = message;
  el.status.classList.toggle("error", isError);
}

/** Geocoding picks the first hit, so the page shows where that landed. */
function showLocated(el: Elements, location: Config["location"]): void {
  const place = [location.name, location.region, location.country].filter(Boolean).join(", ");
  const fix = `${location.lat.toFixed(3)}, ${location.lon.toFixed(3)}`;
  el.located.textContent = `${place} \u00b7 ${fix}`;
}

function fillForm(el: Elements, bands: BandPicker, config: Config): void {
  el.city.value = config.location.name;
  showLocated(el, config.location);
  bands.set(config.thresholds);
  el.rain.value = String(config.rainThresholdPct);
  el.refresh.value = String(config.refreshMinutes);
  updateRain(el);
}

function updateRain(el: Elements): void {
  el.rainOut.textContent = `${el.rain.value}%`;
}

function readForm(el: Elements, bands: BandPicker): ConfigSubmit {
  return {
    thresholds: bands.values(),
    rainThresholdPct: Number(el.rain.value),
    refreshMinutes: Number(el.refresh.value),
    locationName: el.city.value.trim(),
  };
}

async function sync(el: Elements, bands: BandPicker): Promise<void> {
  const server = await fetchState();
  const client = loadClientState();

  if (server.config !== null && server.config.revision > 0) {
    saveClientState({ revision: server.config.revision, config: server.config });
    fillForm(el, bands, server.config);
    return;
  }

  if (client !== null && client.revision > 0) {
    const saved = await submitConfig({
      thresholds: client.config.thresholds,
      rainThresholdPct: client.config.rainThresholdPct,
      refreshMinutes: client.config.refreshMinutes,
      locationName: client.config.location.name,
    });
    saveClientState({ revision: saved.revision, config: saved });
    fillForm(el, bands, saved);
    return;
  }

  el.banner.classList.remove("hidden");
}

async function onSave(el: Elements, bands: BandPicker, event: SubmitEvent): Promise<void> {
  event.preventDefault();
  el.save.disabled = true;
  setStatus(el, "saving...");
  try {
    const config = await submitConfig(readForm(el, bands));
    saveClientState({ revision: config.revision, config });
    fillForm(el, bands, config);
    setStatus(el, "saved");
    el.banner.classList.add("hidden");
  } catch (err) {
    setStatus(el, err instanceof Error ? err.message : "save failed", true);
  } finally {
    el.save.disabled = false;
  }
}

function main(): void {
  const el = elements();
  const bands = new BandPicker(
    {
      track: el.bands,
      low: el.handleLow,
      high: el.handleHigh,
      ranges: [el.bandJacket, el.bandPullover, el.bandShirt],
    },
    () => {
      setStatus(el, "");
      keepAwake();
    },
  );
  el.rain.addEventListener("input", () => updateRain(el));
  el.form.addEventListener("input", () => keepAwake());
  el.form.addEventListener("submit", (event) => void onSave(el, bands, event));
  watchAwake(el);
  sync(el, bands).catch((err: unknown) => {
    setStatus(el, err instanceof Error ? err.message : "device unreachable", true);
  });
}

main();
