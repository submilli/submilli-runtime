// expect-error: syntax nesting exceeds the compiler limit of 256 levels
// A chain rooted in `?.` is one node, but code generation nests every link.
// Each `.f()` is two links: the method and its call.
class N {
  f(): N {
    return this;
  }
}

function main(): number {
  const x: N | null = new N();
  const y = x?.f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f().f();
  return y === null ? 0 : 1;
}
