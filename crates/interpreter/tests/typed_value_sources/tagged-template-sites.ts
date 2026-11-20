// Each `site(n)` call stands for a tagged template's template object; the
// test replaces each call with its own `TemplateObject` node for site `n`'s
// strings, and each `deleteMarker` call with a `DeleteField`.
function site(n: number): readonly string[] {
  return [];
}

function deleteMarker(o: unknown, key: string): boolean {
  return true;
}

function eraseRaw(o: { raw?: readonly string[]; length: number }): boolean {
  return deleteMarker(o, "raw");
}

class Tags {
  prefix: string = ">";
  tag(strings: readonly string[], x: number): string {
    return this.prefix + strings[0] + x + strings[1];
  }
}

function main(): string {
  const out: string[] = [];
  const seen: (readonly string[])[] = [];
  for (let i = 0; i < 3; i++) {
    seen.push(site(1));
  }
  out.push(String(Object.is(seen[0], seen[2])));
  const make = (): readonly string[] => site(1);
  out.push(String(Object.is(make(), make())));
  out.push(String(Object.is(make(), seen[0])));
  const u = site(2);
  const missing: unknown = u[0];
  out.push(String(missing === undefined));
  out.push(JSON.stringify(u));
  out.push(new Tags().tag(site(3), 7));
  try {
    eraseRaw(u);
    out.push("deleted");
  } catch (e) {
    out.push(e instanceof TypeError ? "TypeError" : "other");
  }
  out.push(String("raw" in u));
  return out.join("|");
}
