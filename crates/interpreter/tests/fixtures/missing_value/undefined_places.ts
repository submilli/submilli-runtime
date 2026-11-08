// SUB-2148: a missing read that goes straight into a place whose declared type
// names `undefined` takes `undefined`: a `| undefined` return or variable, an
// optional parameter, an optional field. `unknown` doesn't count.
// expect-warning: `??` on non-nullable type `number` — right side is unreachable
interface Item {
  name: string;
}

interface Box {
  value?: string;
}

const someArray: number[] = [1];

function priceOrNone(item: Item): number | undefined {
  return someArray[100];
}

function price(item: Item): number {
  return someArray[100];
}

function show(g?: string[]): string {
  return g === undefined ? "none" : g.join(",");
}

function optionalGeneric<T>(x?: T): string {
  return x === undefined ? "none" : "some";
}

class Holder {
  value?: string;
}

function takes(x: unknown): string {
  return String(x);
}

function main(): void {
  const item: Item = { name: "x" };
  const groups: Record<string, string[]> = {};
  const seen: Record<string, Item> = {};

  assert(priceOrNone(item) === undefined, "a return typed `number | undefined`");
  assert((priceOrNone(item) ?? 0) === 0, "price(item) ?? 0 with `number | undefined`");
  let threw = false;
  try {
    console.log(price(item) ?? 0);
  } catch (e: MissingValueError) {
    threw = true;
  }
  assert(threw, "price(item) ?? 0 with `number` throws inside price");

  const v: Item | undefined = seen["key"];
  assert(v === undefined, "an annotated `| undefined` variable");
  assert(show(groups["k"]) === "none", "an optional parameter");
  const box: Box = { value: ({} as Record<string, string>)["k"] };
  assert(box.value === undefined, "an optional field");

  const arrow = (): number | undefined => someArray[100];
  assert(arrow() === undefined, "an expression-bodied arrow with a declared return");
  assert(optionalGeneric(seen["key"]) === "none", "a generic optional parameter");
  const holder = new Holder();
  holder.value = ({} as Record<string, string>)["k"];
  assert(holder.value === undefined, "an assignment to an optional field");
  const values: (string | undefined)[] = [];
  values.push(({} as Record<string, string>)["k"]);
  assert(values.length === 1 && values[0] === undefined, "a built-in slot instantiated with `undefined`");

  let unknownThrew = false;
  try {
    console.log(takes(seen["key"]));
  } catch (e: MissingValueError) {
    unknownThrew = true;
  }
  assert(unknownThrew, "an `unknown` parameter does not take a missing value");
}
