// Source order `z, m, a` deliberately differs from sorted order `a, m, z`, so a
// narrow-region wrap that leaned on either one would swap the shadow slots.
function join3(z: string | null, m: string | null, a: string | null): string {
  if (z !== null && m !== null && a !== null) {
    return (
      z + "|" + m + "|" + a + "|" +
      z.length.toString() + m.length.toString() + a.length.toString()
    );
  }
  return "none";
}

function main(): void {
  assert(join3("zz", "m", "aaa") === "zz|m|aaa|213", "all three narrowed");
  assert(join3(null, "m", "aaa") === "none", "first null");
  assert(join3("zz", null, "aaa") === "none", "second null");
  assert(join3("zz", "m", null) === "none", "third null");
}
