// A top-level `const` root: the seeded source is a `GlobalRef`, so nothing is
// captured at all. This is the shape a shadow-capturing design could not
// express — the capture pass walks top-level statements with no frames.
const G: string | null = "global";

function main(): void {
  if (G !== null) {
    const f = (bump: number): number => G.length + bump;
    assert(f(0) === 6, "top-level const narrowed inside the closure");
  } else {
    assert(false, "G is non-null");
  }
}
