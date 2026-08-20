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
  preview: HTMLElement;
  frame: HTMLImageElement;
  previewNote: HTMLElement;
}

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
    preview: document.getElementById("preview") as HTMLElement,
    frame: document.getElementById("frame") as HTMLImageElement,
    previewNote: document.getElementById("previewNote") as HTMLElement,
  };
}

async function refreshFrame(el: Elements): Promise<void> {
  el.previewNote.textContent = "fetching...";
  try {
    const res = await fetch(`/api/frame.bmp?t=${Date.now()}`);
    if (!res.ok) throw new Error((await res.text()) || `frame fetch failed: ${res.status}`);
    el.frame.src = URL.createObjectURL(await res.blob());
    el.previewNote.textContent = "";
  } catch (err) {
    el.previewNote.textContent = err instanceof Error ? err.message : "frame unavailable";
  }
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

function updateOutputs(el: Elements): void {
  el.lowOut.textContent = `${el.low.value}\u00b0C`;
  el.midOut.textContent = `${el.mid.value}\u00b0C`;
  el.highOut.textContent = `${el.high.value}\u00b0C`;
  el.rainOut.textContent = `${el.rain.value}%`;
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
    el.preview.classList.remove("hidden");
    void refreshFrame(el);
    window.setInterval(() => void refreshFrame(el), 60_000);
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
    setStatus(el, "saved, restarting the device...");
    el.banner.classList.add("hidden");
  } catch (err) {
    setStatus(el, err instanceof Error ? err.message : "save failed", true);
  } finally {
    el.save.disabled = false;
  }
}

function main(): void {
  const el = elements();
  for (const input of [el.low, el.mid, el.high, el.rain]) {
    input.addEventListener("input", () => updateOutputs(el));
  }
  el.form.addEventListener("submit", (event) => void onSave(el, event));
  el.preview.addEventListener("click", () => void refreshFrame(el));
  sync(el).catch((err: unknown) => {
    setStatus(el, err instanceof Error ? err.message : "device unreachable", true);
  });
}

main();
