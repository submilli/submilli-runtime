function mine(): number { interface Row { b: string; } const r: Row = { b: "x" }; return r.b.length; }

/** The other module's row. */
export function other(): number {
  return mine();
}
