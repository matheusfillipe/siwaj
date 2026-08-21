/**
 * Two dividers over one temperature axis, cutting it into the three garments.
 * Pointer events rather than stacked range inputs, so each divider sits on the
 * band it splits and stays grabbable on a touch screen.
 */
import type { Thresholds } from "./generated/Thresholds";

/** Mirrors `THRESHOLD_MIN_C`/`MAX_C`, which `Config::validate` enforces. */
const AXIS_MIN = -10;
const AXIS_MAX = 30;
/** The device rejects thresholds that are not strictly ordered. */
const STEP = 0.5;

interface Parts {
  /// The bar itself, which is what pointer positions are measured against.
  track: HTMLElement;
  bands: [HTMLElement, HTMLElement, HTMLElement];
  low: HTMLElement;
  high: HTMLElement;
  values: [HTMLElement, HTMLElement];
}

function snap(value: number): number {
  return Math.round(value / STEP) * STEP;
}

function degrees(value: number): string {
  return `${value}°C`;
}

function percent(value: number): number {
  return ((value - AXIS_MIN) / (AXIS_MAX - AXIS_MIN)) * 100;
}

/// Roughly one readout wide. Closer than this and two centred labels overlap
/// into an unreadable smear, so each one points away from the other instead.
const LABEL_CLEARANCE_PX = 68;

export class BandPicker {
  private thresholds: Thresholds = { lowC: 8, highC: 18 };

  constructor(
    private parts: Parts,
    private onChange: () => void,
  ) {
    this.bind(parts.low, "lowC");
    this.bind(parts.high, "highC");
    parts.track.addEventListener("pointerdown", (event) => this.grabNearest(event));
    this.render();
  }

  values(): Thresholds {
    return { ...this.thresholds };
  }

  set(next: Thresholds): void {
    this.thresholds = { ...next };
    this.clamp("lowC");
    this.render();
  }

  private clamp(moved: keyof Thresholds): void {
    const { lowC, highC } = this.thresholds;
    if (moved === "lowC") {
      this.thresholds.highC = Math.max(highC, lowC + STEP);
    } else {
      this.thresholds.lowC = Math.min(lowC, highC - STEP);
    }
    this.thresholds.lowC = Math.min(Math.max(this.thresholds.lowC, AXIS_MIN), AXIS_MAX - STEP);
    this.thresholds.highC = Math.min(Math.max(this.thresholds.highC, AXIS_MIN + STEP), AXIS_MAX);
  }

  private move(key: keyof Thresholds, value: number): void {
    this.thresholds[key] = snap(value);
    this.clamp(key);
    this.render();
    this.onChange();
  }

  private valueAt(clientX: number): number {
    const box = this.parts.track.getBoundingClientRect();
    const ratio = (clientX - box.left) / box.width;
    return AXIS_MIN + Math.min(Math.max(ratio, 0), 1) * (AXIS_MAX - AXIS_MIN);
  }

  /** A tap anywhere moves the nearer divider, so the whole bar is a target. */
  private grabNearest(event: PointerEvent): void {
    if (event.target !== this.parts.track && (event.target as HTMLElement).closest(".handle")) {
      return;
    }
    const value = this.valueAt(event.clientX);
    const key: keyof Thresholds =
      Math.abs(value - this.thresholds.lowC) <= Math.abs(value - this.thresholds.highC)
        ? "lowC"
        : "highC";
    this.move(key, value);
    (key === "lowC" ? this.parts.low : this.parts.high).focus();
  }

  private bind(handle: HTMLElement, key: keyof Thresholds): void {
    let dragging = false;
    const end = (): void => {
      dragging = false;
    };
    handle.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      dragging = true;
      // capture routes moves that leave the handle back to it; the flag is
      // what decides whether to act on them, so a refused capture still drags
      if (handle.setPointerCapture) handle.setPointerCapture(event.pointerId);
      handle.focus();
    });
    handle.addEventListener("pointermove", (event) => {
      if (!dragging) return;
      this.move(key, this.valueAt(event.clientX));
    });
    handle.addEventListener("pointerup", end);
    handle.addEventListener("pointercancel", end);
    handle.addEventListener("keydown", (event) => {
      const by = { ArrowLeft: -STEP, ArrowRight: STEP, ArrowDown: -STEP, ArrowUp: STEP }[
        event.key
      ];
      if (by === undefined) return;
      event.preventDefault();
      this.move(key, this.thresholds[key] + by);
    });
  }

  private render(): void {
    const { lowC, highC } = this.thresholds;
    const spans = [lowC - AXIS_MIN, highC - lowC, AXIS_MAX - highC];
    this.parts.bands.forEach((band, i) => {
      band.style.flexGrow = String(Math.max(spans[i] ?? 0, 0));
    });
    const dividers = [
      [this.parts.low, this.parts.values[0], lowC],
      [this.parts.high, this.parts.values[1], highC],
    ] as const;
    for (const [handle, label, value] of dividers) {
      handle.style.left = `${percent(value)}%`;
      handle.setAttribute("aria-valuemin", String(AXIS_MIN));
      handle.setAttribute("aria-valuemax", String(AXIS_MAX));
      handle.setAttribute("aria-valuenow", String(value));
      handle.setAttribute("aria-valuetext", degrees(value));
      label.textContent = degrees(value);
    }
    this.spaceLabels(highC - lowC);
  }

  private spaceLabels(spread: number): void {
    const width = this.parts.track.getBoundingClientRect().width;
    if (width === 0) return;
    const apart = (spread / (AXIS_MAX - AXIS_MIN)) * width >= LABEL_CLEARANCE_PX;
    this.parts.low.classList.toggle("crowded", !apart);
    this.parts.high.classList.toggle("crowded", !apart);
  }
}
