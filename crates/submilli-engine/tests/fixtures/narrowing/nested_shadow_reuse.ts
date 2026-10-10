// Nested and sibling narrow regions over the same two roots, plus a closure that
// re-narrows both: every `#narrow_N` shadow must resolve to its own slot.
interface Node {
  zed: string | null;
  alpha: string | null;
}

function outer(a1: Node | null, b1: Node | null): string {
  const n: Node | null = a1;
  const m: Node | null = b1;
  let out: string = "";
  if (n !== null && m !== null) {
    if (n.zed !== null && n.alpha !== null) {
      out = out + n.zed + n.alpha;
      if (m.zed !== null && m.alpha !== null) {
        out = out + m.zed + m.alpha;
        const f = (): string => {
          const nz: string | null = n.zed;
          const ma: string | null = m.alpha;
          if (nz !== null && ma !== null) {
            return nz + ma;
          }
          return "?";
        };
        out = out + f();
      }
    }
    if (m.zed !== null && n.alpha !== null) {
      out = out + m.zed + n.alpha;
    }
  }
  return out;
}

function main(): void {
  const a: Node = { zed: "z", alpha: "a" };
  const b: Node = { zed: "Z", alpha: "A" };
  assert(outer(a, b) === "zaZAzAZa", "nested shadows resolve to the right locals");
  assert(outer(a, null) === "", "m null");
  assert(outer({ zed: null, alpha: "a" }, b) === "Za", "only the second block runs");
}
