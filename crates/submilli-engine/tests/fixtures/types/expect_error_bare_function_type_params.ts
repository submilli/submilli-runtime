// expect-error: function-type params require named annotations in v1
// `(T)` is spelled identically as a grouped type and as a bare parameter list, and only
// a trailing `=>` tells them apart. Here that `=>` is the type's own, so `(T)` routes to
// the function-type parse and this message stays reachable. In an arrow's return
// annotation the `=>` belongs to the body, so `(T)` groups there instead.
type Bad = (T) => number;

function main(): void {
  const f: Bad = (n: number): number => n;
  f(1);
}
