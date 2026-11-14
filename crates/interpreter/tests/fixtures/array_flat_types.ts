enum Depth { One = 1, Two = 2 }
enum StopDepth { End = -1 }
enum OnlyTwo { Depth = 2 }
function flatten(xs: number[][][], depth: number): (number | number[] | number[][])[] {
  return xs.flat(depth);
}
function varying(xs: number[][][], depth: 0 | 2): (number | number[] | number[][])[] {
  return xs.flat(depth);
}
function readonlyFlat(xs: readonly (readonly number[])[]): number[] {
  return xs.flat();
}
function genericFlat<T>(xs: T[][]): T[] {
  return xs.flat();
}
function stopFlat<T>(value: FlatArray<T, -1>): T { return value; }
function stopEnum<T>(value: FlatArray<T, StopDepth.End>): T { return value; }
function noDepth<T>(value: FlatArray<T, never>): never { return value; }
function main(): void {
  const xs: number[][][] = [[[1, 2]], [[3]]];
  const one: number[][] = xs.flat();
  const two: number[] = xs.flat(2);
  const deep: number[] = xs.flat(21);
  const dynamic = flatten(xs, 2);
  assert(JSON.stringify(one) === "[[1,2],[3]]", "default depth");
  assert(JSON.stringify(two) === "[1,2,3]", "literal depth");
  assert(JSON.stringify(deep) === "[1,2,3]", "depth 21");
  assert(JSON.stringify(dynamic) === "[1,2,3]", "dynamic depth");
  const tuple: [number[], string[]] = [[4], ["five"]];
  const mixed: (number | string)[] = tuple.flat();
  assert(JSON.stringify(mixed) === '[4,"five"]', "tuple elements distribute");
  const union: (number[] | string)[] = [[6], "seven"];
  const joined: (number | string)[] = union.flat();
  assert(JSON.stringify(joined) === '[6,"seven"]', "union elements distribute");
  const explicit: number[] = xs.flat<number[][][], 2>(2);
  const defaulted: number[][] = xs.flat<number[][][]>();
  const missing: (number | number[] | number[][])[] = xs.flat(undefined);
  const varied = varying(xs, 0);
  assert(JSON.stringify(explicit) === "[1,2,3]", "explicit type arguments");
  assert(JSON.stringify(defaulted) === "[[1,2],[3]]", "defaulted type argument");
  assert(JSON.stringify(missing) === "[[1,2],[3]]", "undefined uses runtime default");
  assert(JSON.stringify(varied) === "[[[1,2]],[[3]]]", "union depth zero");
  assert(JSON.stringify(readonlyFlat([[8], [9]])) === "[8,9]", "readonly arrays");
  const negative: number[][][][] = xs.flat(-1);
  const fractional: number[] = xs.flat(1.5);
  assert(JSON.stringify(negative) === "[[[1,2]],[[3]]]", "negative runtime depth is zero");
  assert(JSON.stringify(fractional) === "[[1,2],[3]]", "fractional runtime depth truncates");
  assert(JSON.stringify(genericFlat([[10], [11]])) === "[10,11]", "generic element type");
  const enumOne: number[][] = xs.flat(Depth.One);
  const enumTwo: number[] = xs.flat(Depth.Two);
  assert(JSON.stringify(enumOne) === "[[1,2],[3]]", "enum depth one");
  assert(JSON.stringify(enumTwo) === "[1,2,3]", "enum depth two");
  assert(stopFlat<number>(12) === 12, "generic stop case");
  assert(stopEnum<number>(13) === 13, "enum generic stop case");
  const singleton: number[] = xs.flat(OnlyTwo.Depth);
  assert(JSON.stringify(singleton) === "[1,2,3]", "singleton enum depth");
  const utility: FlatArray<number[][][], 1>[] = one;
  assert(utility.length === 2, "public FlatArray alias");
}
