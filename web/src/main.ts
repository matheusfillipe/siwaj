import { fetchState, submitConfig } from "./api";
import { loadClientState, saveClientState } from "./store";
import type { Config } from "./generated/Config";
import type { ConfigSubmit } from "./generated/ConfigSubmit";

interface Elements {
  banner: HTMLElement;
  status: HTMLElement;
  form: HTMLFormElement;
  city: HTMLInputElement;
  low: HTMLInputElement;
  mid: HTMLInputElement;
  high: HTMLInputElement;
  rain: HTMLInputElement;
  refresh: HTMLSelectElement;
  save: HTMLButtonElement;
  lowOut: HTMLOutputElement;
  midOut: HTMLOutputElement;
  highOut: HTMLOutputElement;
  rainOut: HTMLOutputElement;
  bands: HTMLElement;
  bandJacket: HTMLElement;
  bandPullover: HTMLElement;
  bandShirt: HTMLElement;
  bandTshirt: HTMLElement;
}

/// The band bar spans the slider range, so a threshold's position on the
/// track and its edge in the bar are the same number.
const AXIS_MIN = -10;
const AXIS_MAX = 30;
/// Thresholds must stay strictly ordered or the device rejects the config;
/// one slider step of separation is the smallest gap that survives rounding.
const STEP = 0.5;

function elements(): Elements {
  return {
    banner: document.getElementById("banner") as HTMLElement,
    status: document.getElementById("status") as HTMLElement,
    form: document.getElementById("config-form") as HTMLFormElement,
    city: document.getElementById("city") as HTMLInputElement,
    low: document.getElementById("low") as HTMLInputElement,
    mid: document.getElementById("mid") as HTMLInputElement,
    high: document.getElementById("high") as HTMLInputElement,
    rain: document.getElementById("rain") as HTMLInputElement,
    refresh: document.getElementById("refresh") as HTMLSelectElement,
    save: document.getElementById("save") as HTMLButtonElement,
    lowOut: document.getElementById("lowOut") as HTMLOutputElement,
    midOut: document.getElementById("midOut") as HTMLOutputElement,
    highOut: document.getElementById("highOut") as HTMLOutputElement,
    rainOut: document.getElementById("rainOut") as HTMLOutputElement,
    bands: document.getElementById("bands") as HTMLElement,
    bandJacket: document.getElementById("bandJacket") as HTMLElement,
    bandPullover: document.getElementById("bandPullover") as HTMLElement,
    bandShirt: document.getElementById("bandShirt") as HTMLElement,
    bandTshirt: document.getElementById("bandTshirt") as HTMLElement,
  };
}

function setStatus(el: Elements, message: string, isError = false): void {
  el.status.textContent = message;
  el.status.classList.toggle("error", isError);
}

function fillForm(el: Elements, config: Config): void {
  el.city.value = config.location.name;
  el.low.value = String(config.thresholds.lowC);
  el.mid.value = String(config.thresholds.midC);
  el.high.value = String(config.thresholds.highC);
  el.rain.value = String(config.rainThresholdPct);
  el.refresh.value = String(config.refreshMinutes);
  updateOutputs(el);
}

/// Keeps the three handles strictly ordered by pushing the neighbours the
/// dragged one would otherwise pass, so a drag never parks the form in a
/// state the device would reject.
function reorder(el: Elements, dragged: HTMLInputElement): void {
  let [low, mid, high] = [Number(el.low.value), Number(el.mid.value), Number(el.high.value)];
  if (dragged === el.low) {
    mid = Math.max(mid, low + STEP);
    high = Math.max(high, mid + STEP);
  } else if (dragged === el.mid) {
    low = Math.min(low, mid - STEP);
    high = Math.max(high, mid + STEP);
  } else {
    mid = Math.min(mid, high - STEP);
    low = Math.min(low, mid - STEP);
  }
  el.low.value = String(low);
  el.mid.value = String(mid);
  el.high.value = String(high);
}

function degrees(value: number): string {
  return `${value}\u00b0C`;
}

function updateOutputs(el: Elements): void {
  const [low, mid, high] = [Number(el.low.value), Number(el.mid.value), Number(el.high.value)];
  el.lowOut.textContent = degrees(low);
  el.midOut.textContent = degrees(mid);
  el.highOut.textContent = degrees(high);
  el.rainOut.textContent = `${el.rain.value}%`;

  const span = AXIS_MAX - AXIS_MIN;
  const widths = [low - AXIS_MIN, mid - low, high - mid, AXIS_MAX - high];
  const cells = [el.bandJacket, el.bandPullover, el.bandShirt, el.bandTshirt];
  const labels = [`below ${degrees(low)}`, `to ${degrees(mid)}`, `to ${degrees(high)}`, `above`];
  for (const [i, cell] of cells.entries()) {
    const band = cell.parentElement as HTMLElement;
    band.style.flexGrow = String(Math.max(widths[i] ?? 0, 0) / span);
    cell.textContent = labels[i] ?? "";
  }
}

function readForm(el: Elements): ConfigSubmit {
  return {
    thresholds: {
      lowC: Number(el.low.value),
      midC: Number(el.mid.value),
      highC: Number(el.high.value),
    },
    rainThresholdPct: Number(el.rain.value),
    refreshMinutes: Number(el.refresh.value),
    locationName: el.city.value.trim(),
  };
}

async function sync(el: Elements): Promise<void> {
  const server = await fetchState();
  const client = loadClientState();

  if (server.config !== null && server.config.revision > 0) {
    saveClientState({ revision: server.config.revision, config: server.config });
    fillForm(el, server.config);
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
    fillForm(el, saved);
    return;
  }

  el.banner.classList.remove("hidden");
}

async function onSave(el: Elements, event: SubmitEvent): Promise<void> {
  event.preventDefault();
  el.save.disabled = true;
  setStatus(el, "saving...");
  try {
    const config = await submitConfig(readForm(el));
    saveClientState({ revision: config.revision, config });
    fillForm(el, config);
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
  for (const input of [el.low, el.mid, el.high]) {
    input.addEventListener("input", () => {
      reorder(el, input);
      updateOutputs(el);
    });
  }
  el.rain.addEventListener("input", () => updateOutputs(el));
  el.form.addEventListener("submit", (event) => void onSave(el, event));
  sync(el).catch((err: unknown) => {
    setStatus(el, err instanceof Error ? err.message : "device unreachable", true);
  });
}

main();
