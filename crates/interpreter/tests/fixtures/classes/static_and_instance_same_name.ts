// A class may declare a static and an instance member of the same name. An
// instance receiver resolves the instance one — through `.` and `?.` alike, so
// the "that's a static" diagnostic must be checked only after instance members.
class K {
  static dup(): string {
    return "static";
  }
  dup(): string {
    return "instance";
  }
  static readonly TAG: string = "tag";
}

function main(): void {
  const k = new K();
  assert(k.dup() === "instance", "instance method wins over the same-named static");
  assert(K.dup() === "static", "the static is still reachable on the class");

  const maybe: K | null = k;
  const viaChain = maybe?.dup();
  assert(viaChain === "instance", "same, through an optional chain");

  assert(K.TAG === "tag");
}
