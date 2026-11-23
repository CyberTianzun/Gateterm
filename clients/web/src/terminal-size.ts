export interface Dimensions {
  cols: number;
  rows: number;
}

/** Normalize FitAddon dimensions, rejecting missing, non-finite, or sub-1×1 values. */
export function sanitizeDimensions(
  dims: { cols?: number; rows?: number } | null | undefined,
): Dimensions | null {
  if (!dims) return null;
  const { cols, rows } = dims;
  if (
    typeof cols !== "number" ||
    typeof rows !== "number" ||
    !Number.isFinite(cols) ||
    !Number.isFinite(rows)
  ) {
    return null;
  }
  const flooredCols = Math.floor(cols);
  const flooredRows = Math.floor(rows);
  if (flooredCols < 1 || flooredRows < 1) return null;
  return { cols: flooredCols, rows: flooredRows };
}

export interface RefitScheduler {
  schedule(): void;
  cancel(): void;
}

/** Coalesce bursty layout/font/visibility refit triggers into at most one pending frame. */
export function createRefitScheduler(opts: {
  scheduleFrame: (cb: () => void) => number;
  cancelFrame?: (id: number) => void;
  run: () => void;
}): RefitScheduler {
  let pendingId: number | null = null;
  let scheduled = false;
  let epoch = 0;

  return {
    schedule() {
      if (scheduled) return;
      scheduled = true;
      const token = ++epoch;
      const id = opts.scheduleFrame(() => {
        if (epoch !== token) return;
        scheduled = false;
        pendingId = null;
        opts.run();
      });
      if (scheduled && epoch === token) {
        pendingId = id;
      }
    },
    cancel() {
      epoch++;
      if (!scheduled) return;
      scheduled = false;
      const id = pendingId;
      pendingId = null;
      if (id !== null) opts.cancelFrame?.(id);
    },
  };
}
