// `Parameters` and `ReturnType` of a generic function type read its own type
// parameters as their bounds, or `unknown`, and keep a generic result
// generic, as tsc does.
type Bounded = <T extends string>(x: T) => T;
type Maker = () => <U>(y: U) => U;

function main(): void {
  const a: ReturnType<Bounded> = "s";
  const n: string = a;
  const p: Parameters<Bounded> = ["t"];
  const first: string = p[0];
  const r: ReturnType<Maker> = <U,>(y: U): U => y;
  assert(n === "s" && first === "t" && r<number>(3) === 3, "bounds and a generic result");
}
