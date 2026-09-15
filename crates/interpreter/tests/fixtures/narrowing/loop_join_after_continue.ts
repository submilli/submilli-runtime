// Two sibling narrowings per loop body, each established by a `continue` guard, so
// the post-continue join has to carry both paths in both loop forms.
interface Rec {
  zed: string | null;
  alpha: string | null;
}

function scan(rs: Rec[]): string {
  let out: string = "";
  for (const r of rs) {
    if (r.zed === null || r.alpha === null) {
      continue;
    }
    out = out + r.zed + r.alpha;
  }
  let i: number = 0;
  while (i < rs.length) {
    const r: Rec = rs[i];
    i = i + 1;
    if (r.alpha === null) {
      continue;
    }
    if (r.zed === null) {
      continue;
    }
    out = out + r.alpha + r.zed;
  }
  return out;
}

function main(): void {
  const rs: Rec[] = [
    { zed: "z", alpha: "a" },
    { zed: null, alpha: "b" },
    { zed: "Z", alpha: "A" },
  ];
  assert(scan(rs) === "zaZAazAZ", "post-continue joins keep both sibling narrowings");
}
