export interface Handler {
  run(a: string, b: string, c: string, d: string, e: string, f: string): void;
  fetch(a: string, b: string, c: string, d: string, e: string): string;
}

export function invoke(h: Handler): void {
  h.run("1", "2", "3", "4", "5", "6");
}

/** Same dispatch through an optional chain, whose receiver is nullable. */
export function maybe(h: Handler | null): string | undefined {
  return h?.fetch("1", "2", "3", "4", "5");
}

/** Supplies the `run` field-name global the shape scan needs (SUB-150). */
export function unrelated(): number {
  const other = { run: (x: number): number => x + 1 };
  return other.run(1);
}
