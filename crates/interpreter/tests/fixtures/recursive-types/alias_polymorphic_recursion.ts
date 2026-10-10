// Polymorphic (non-regular) recursion: the back-edge instantiates the alias at
// a *larger* argument than the one it was entered with, so no two levels of the
// expansion are ever spelled the same. Anything that expands a back-edge and
// then walks what it expanded has to stop on the alias *name* — memoizing on
// the type never repeats here, and the expansion grows without bound until the
// compiler runs out of stack.
//
// The types are uninhabited (every value would need an infinitely deep `next`),
// so they can only appear in signatures. Declaring them is enough: every
// signature is walked for the shapes it reaches, whether or not it is called.
type Grow<T> = { v: T; next: Grow<Grow<T>> };
type Boxed<T> = { a: T; b: Boxed<T[]> };

// Mutual polymorphic recursion — the argument grows around a two-name cycle.
type Ping<T> = { x: T; pong: Pong<T> };
type Pong<T> = { y: T; ping: Ping<T[]> };

// A `| null` base case, instantiated at a *nested* argument in a signature.
// That is what binds a type parameter to a type still mentioning that same
// parameter, which substitution has to stop re-entering rather than chase.
// (Annotating a *value* at the nested instantiation still overflows — SUB-817.)
type Chain<T> = { v: T; next: Chain<Chain<T>> | null };

function useGrow(g: Grow<number>): number {
  return g.v;
}

// The generic form: the parameter is still open when the alias is instantiated.
function useGrowGeneric<T>(g: Grow<T>): number {
  return 1;
}

function useBoxed(b: Boxed<number>): number {
  return b.a;
}

function usePing(p: Ping<number>): number {
  return p.x;
}

function useChain<T>(c: Chain<Chain<T>>): number {
  return 1;
}

function main(): void {
  const tip: Chain<number> = { v: 1, next: null };
  assert(tip.v === 1, "a polymorphic-recursive value at its base case");
}
