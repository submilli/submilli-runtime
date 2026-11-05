// A canonical numeric string key reads the number index, as in TypeScript:
// `s["0"]` is `s[0]`. Other string keys still name properties.
// Any number's own spelling is a numeric key, as tsc reads one: `"-1"`
// and `"1.5"` read an element type, as `arr[-1]` does (and trap the same
// way, out of range), while `"01"` names a field.
function unusualKeys(arr: number[], text: string): number {
  const negative: number = arr["-1"];
  const fraction: number = arr["1.5"];
  const char: string = text["-1"];
  return negative + fraction + char.length;
}

// A tuple in a union with an array reads any element at a key with no
// slot, the number index every member has.
function unionIndexKeys(u: [number, string] | number[]): number | string {
  return u["1.5"];
}

function optionalRead(xs: number[] | undefined): number | undefined { return xs?.["0"]; }

function main(): void {
  const s = "héllo";
  const first: string = s["0"];
  assert(first === "h", "a string");
  assert(`a${1}b`[`2`] === "b", "a template key on a template string");

  const xs: number[] = [10, 20];
  const second: number = xs["1"];
  assert(second === 20, "an array");

  assert(xs?.["0"] === 10, "optional numeric key");
  xs["0"] = 30;
  xs["0"] += 2;
  xs["0"]++;
  assert(xs[0] === 33, "numeric string writes and updates");
  const absent: number[] | undefined = undefined;
  assert(optionalRead(absent) === undefined, "optional absent receiver");

  const bytes = new Uint8Array([1, 2]);
  assert(bytes?.["0"] === 1, "optional typed-array read");
  bytes["0"] = 3;
  bytes["0"] += 2;
  bytes["0"]++;
  assert(bytes[0] === 6, "typed-array writes and updates");

  const pair: [number, string] = [1, "x"];
  const tail: string = pair["1"];
  assert(pair?.["1"] === "x", "optional tuple slot");
  assert(tail === "x", "a tuple slot keeps its own type");

  const either: string[] | number[] = s.length > 0 ? ["a"] : [1];
  const firstOfEither: string | number = either["0"];
  assert(firstOfEither === "a", "a union of arrays");

  assert(s["length"] === 5, "a non-numeric key is a property");
}
