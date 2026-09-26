interface Box {
  n: number;
}

function boom(): boolean {
  assert(false, "rhs must not evaluate when the lhs short-circuits");
  return true;
}

function main(): void {
  const b: Box | null = { n: 7 };
  const v: number | null = b && b.n;
  assert(v === 7, "truthy object yields the rhs field");
  const nb: Box | null = null as Box | null;
  const v2: number | null = nb && nb.n;
  assert(v2 === null, "null lhs short-circuits to null");

  const r: null | boolean = null && boom();
  assert(r === null, "&& keeps the falsy null lhs");
  const s: number | boolean = 0 && boom();
  assert((s as number) === 0, "&& keeps the falsy zero lhs");
  const t: string | boolean = ("" && boom()) as string | boolean;
  assert((t as string) === "", "&& keeps the empty-string lhs");
  assert(true || boom(), "|| short-circuits on a truthy lhs");

  const n: number = NaN;
  const w: number | boolean = n && true;
  assert(isNaN(w as number), "NaN lhs short-circuits and is returned");

  const u: boolean = true && true;
  assert(u, "boolean && boolean still types as boolean");
}
