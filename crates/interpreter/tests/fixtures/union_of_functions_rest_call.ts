// A union of function types with rest parameters is called as in tsc: through
// a member whose parameters every other member accepts, or else through one
// combined signature, in which a rest parameter covers every later position.
type Fixed = (a: string, b?: string) => string;
type Rest = (a: string, ...rest: string[]) => string;
type OptionalThenRest = (a: string, b?: string, ...rest: string[]) => string;
type Pair = (a: string, b: string) => string;

const fixed: Fixed = (a: string, b?: string) => `fixed ${a} ${b ?? "-"}`;
const rest: Rest = (a: string, ...more: string[]) => `rest ${a} [${more.join(",")}]`;
const optionalThenRest: OptionalThenRest = (a: string, b?: string, ...more: string[]) =>
  `opt ${a} ${b ?? "-"} [${more.join(",")}]`;
const pair: Pair = (a: string, b: string) => `pair ${a} ${b}`;

function restOrOptional(f: Rest | OptionalThenRest): string {
  return [f("a"), f("a", "b"), f("a", "b", "c")].join("; ");
}

// An omitted optional argument isn't passed to the rest member as undefined.
function optionalOrRest(f: ((a: number, b?: number) => string) | ((...xs: number[]) => string)): string {
  return [f(1), f(1, 2)].join("; ");
}

function fixedOrRest(f: Fixed | Rest): string {
  return [f("x"), f("x", "y", "z", "w")].join("; ");
}

function restOrPair(f: Rest | Pair): string {
  return f("p", "q");
}

interface Holder {
  call: Rest | Pair;
  maybe?: Rest | OptionalThenRest;
}

function main(): void {
  assert(restOrOptional(rest) === "rest a []; rest a [b]; rest a [b,c]");
  assert(restOrOptional(optionalThenRest) === "opt a - []; opt a b []; opt a b [c]");
  assert(optionalOrRest((...xs: number[]) => `n=${xs.length}`) === "n=1; n=2");
  assert(optionalOrRest((a: number, b?: number) => `${a} ${b ?? "-"}`) === "1 -; 1 2");
  assert(fixedOrRest(fixed) === "fixed x -; fixed x y");
  assert(fixedOrRest(rest) === "rest x []; rest x [y,z,w]");
  assert(restOrPair(rest) === "rest p [q]");
  assert(restOrPair(pair) === "pair p q");

  const holders: Holder[] = [{ call: rest, maybe: optionalThenRest }, { call: pair }];
  const seen: string[] = [];
  for (const holder of holders) {
    seen.push(holder.call("m", "n"));
    seen.push(holder.maybe?.("x", "y", "z") ?? "none");
  }
  assert(seen.join("; ") === "rest m [n]; opt x y [z]; pair m n; none");
}
