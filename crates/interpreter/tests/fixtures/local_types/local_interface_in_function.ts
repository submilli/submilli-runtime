// An interface declared inside a function is in scope for the whole function
// body (SUB-2093).

interface Cand { id: string; nc: number; }

function main(): void {
  interface Kept { c: Cand; noul: number; }
  const kept: Kept[] = [];
  kept.push({ c: { id: "a", nc: 1 }, noul: 0.5 });
  kept.push({ c: { id: "b", nc: 3 }, noul: 0.5 });
  kept.push({ c: { id: "c", nc: 2 }, noul: 0.9 });
  kept.sort((x, y) => (y.noul - x.noul) || (y.c.nc - x.c.nc));
  const top: Kept[] = kept.slice(0, 2);
  assert(top.map(k => k.c.id).join(",") === "c,b", "sorted by noul, then nc");
}
