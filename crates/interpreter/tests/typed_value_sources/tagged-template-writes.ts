// Each `site(n)` call stands for a tagged template's template object; the
// test replaces it with a `TemplateObject` node for site `n`.
function site(n: number): readonly string[] {
  return [];
}

function throwsTypeError(write: () => void): string {
  try {
    write();
    return "wrote";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

function put<T, K extends keyof T>(o: T, k: K, v: T[K]): void {
  o[k] = v;
}

function get<T, K extends keyof T>(o: T, k: K): T[K] {
  return o[k];
}

function main(): string {
  const out: string[] = [];
  const one = site(1) as string[];
  out.push(throwsTypeError(() => { one.sort(); }));
  out.push(throwsTypeError(() => { one.reverse(); }));
  const s = site(2);
  const m = s as string[];
  out.push(throwsTypeError(() => { m.unshift(); }));
  out.push(throwsTypeError(() => { m.splice(0, 0); }));
  out.push(throwsTypeError(() => { m.shift(); }));
  out.push(throwsTypeError(() => { m.fill("x", 0, 1); }));
  out.push(throwsTypeError(() => { m.copyWithin(0, 1); }));
  out.push(throwsTypeError(() => { put(m, 0, "x"); }));
  out.push(throwsTypeError(() => { put(m, "length", 0); }));
  out.push(throwsTypeError(() => { Object.assign(m, { length: 1 }); }));
  const copy = [...s];
  copy.push("x");
  const sliced = s.slice();
  sliced[0] = "y";
  out.push(copy.join(",") + "/" + sliced.join(","));
  const u = site(3);
  const missing: string = u[0];
  out.push(String(missing === undefined));
  out.push(u[0] + "!" + u[1]);
  out.push(throwsTypeError(() => { u[0].toUpperCase(); }));
  const view: unknown = s;
  if (typeof view === "object" && view !== null && "raw" in view) {
    const raw = get(view, "raw");
    out.push(JSON.stringify(raw));
    out.push(throwsTypeError(() => { put(view, "raw", []); }));
    out.push(String(typeof raw === "object" && raw !== null && Object.hasOwn(raw, "raw")));
  }
  out.push(String(Object.hasOwn(s, "Raw")));
  out.push(JSON.stringify(s));
  return out.join("|");
}
