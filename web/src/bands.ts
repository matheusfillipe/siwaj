/// Two dividers over one temperature axis, cutting it into the three
/// garments. Built on pointer events rather than stacked range inputs so the
/// dividers sit on the band they split and stay grabbable on a touch screen.

export const AXIS_MIN = -10;
export const AXIS_MAX = 30;
/// The device rejects thresholds that are not strictly ordered, so the
/// dividers keep at least one step between them.
export const STEP = 0.5;

export interface Thresholds {
  lowC: number;
  highC: number;
}

interface Parts {
  track: HTMLElement;
  low: HTMLElement;
  high: HTMLElement;
  ranges: [HTMLElement, HTMLElement, HTMLElement];
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

export class BandPicker {
  private thresholds: Thresholds = { lowC: 8, highC: 18 };

  constructor(
    private parts: Parts,
    private onChange: () => void,
  ) {
    this.bind(parts.low, "lowC");
    this.bind(parts.high, "highC");
    parts.track.addEventListener("pointerdown", (event) => this.grabNearest(event));
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

  /// A tap on the track moves whichever divider is closer, so the whole bar
  /// is a target instead of just the two handles.
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
    const labels = [`below ${degrees(lowC)}`, `to ${degrees(highC)}`, "above"];
    this.parts.ranges.forEach((cell, i) => {
      const band = cell.parentElement as HTMLElement;
      band.style.flexGrow = String(Math.max(spans[i] ?? 0, 0));
      cell.textContent = labels[i] ?? "";
    });
    for (const [handle, value] of [
      [this.parts.low, lowC],
      [this.parts.high, highC],
    ] as const) {
      handle.style.left = `${percent(value)}%`;
      handle.setAttribute("aria-valuenow", String(value));
      handle.setAttribute("aria-valuetext", degrees(value));
    }
  }
}
