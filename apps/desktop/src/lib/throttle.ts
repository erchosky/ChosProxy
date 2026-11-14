/**
 * Runs `fn` at most once per `ms`: the first call runs right away, calls
 * during the wait are merged into one trailing run. Unlike a debounce, a
 * steady stream of calls still produces regular runs.
 */
export function throttle(fn: () => void, ms: number): () => void {
  let last = Number.NEGATIVE_INFINITY;
  let timer: ReturnType<typeof setTimeout> | undefined;
  return () => {
    const now = Date.now();
    const wait = last + ms - now;
    if (wait <= 0 && timer === undefined) {
      last = now;
      fn();
      return;
    }
    if (timer !== undefined) return;
    timer = setTimeout(
      () => {
        timer = undefined;
        last = Date.now();
        fn();
      },
      Math.max(0, wait),
    );
  };
}
