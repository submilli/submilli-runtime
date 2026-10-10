import ns from "@t/lib";
function main(): void {
  const u: unknown = ns.makeBox() as unknown;
  if (u instanceof ns.Box) { assert(true, "ns.Box instanceof"); } else { assert(false, "wrong branch"); }
  const p: unknown = ns.makePlain() as unknown;
  if (p instanceof ns.Plain) { assert(true, "ns.Plain instanceof"); } else { assert(false, "wrong branch 2"); }
}
