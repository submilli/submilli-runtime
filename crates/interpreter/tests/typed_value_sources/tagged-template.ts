// Each `site(n)` call stands for a tagged template's template object; the
// test replaces it with a `TemplateObject` node for site `n`.
function site(n: number): readonly string[] {
  return [];
}

function first(): readonly string[] {
  return site(1);
}

function second(): readonly string[] {
  return site(2);
}

function tag(strings: readonly string[], x: number, y: number): string {
  return strings[0] + x + strings[1] + y + strings[2];
}

function throwsTypeError(write: () => void): string {
  try {
    write();
    return "wrote";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

function rawOf(value: unknown): unknown {
  if (typeof value === "object" && value !== null && "raw" in value) {
    return value.raw;
  }
  return "missing";
}

function main(): string {
  const s = first();
  const out: string[] = [];
  // `===` compares arrays by structure here, so identity uses `Object.is`.
  out.push(String(Object.is(s, first())));
  out.push(String(Object.is(s, second())));
  out.push(tag(s, 1, 2));
  out.push(String(s.length));
  out.push(JSON.stringify(s));
  out.push(Object.keys(s).join(","));
  const forIn: string[] = [];
  for (const key in s) {
    forIn.push(key);
  }
  out.push(forIn.join(","));
  out.push(String([...s].length));
  out.push(String("raw" in s));
  out.push(String(Object.hasOwn(s, "raw")));
  const raw = rawOf(s);
  out.push(JSON.stringify(raw));
  out.push(String(rawOf(raw)));
  out.push(String(Object.is(raw, rawOf(first()))));
  const m = s as string[];
  out.push(throwsTypeError(() => { m[0] = "x"; }));
  out.push(throwsTypeError(() => { m[3] = "x"; }));
  out.push(throwsTypeError(() => { m[0] += "x"; }));
  out.push(throwsTypeError(() => { m.push("x"); }));
  out.push(throwsTypeError(() => { m.push(); }));
  out.push(throwsTypeError(() => { m.pop(); }));
  out.push(throwsTypeError(() => { m.length = 3; }));
  out.push(throwsTypeError(() => { m.reverse(); }));
  out.push(throwsTypeError(() => { m.sort(); }));
  out.push(throwsTypeError(() => { m.fill("x", 3); }));
  out.push(throwsTypeError(() => { m.copyWithin(0, 3); }));
  out.push(throwsTypeError(() => { Object.assign(m, { 0: "x" }); }));
  const r = raw as string[];
  out.push(throwsTypeError(() => { r[0] = "x"; }));
  out.push(JSON.stringify(s));
  return out.join("|");
}
