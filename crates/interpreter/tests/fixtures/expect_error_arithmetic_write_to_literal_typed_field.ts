// `++`, `--` and compound arithmetic on a field or array element of literal
// type are rejected, where tsc checks them against the base type. Every alias
// reads the field at its literal type, which codegen trusts, so a `0 | 1`
// field holding `2` would miss every case of an exhaustive switch on it.
// expect-error: expected `0 | 1`, got `number`
// expect-error: expected `""`, got `string`
// expect-error: expected `0`, got `number`
// expect-error: expected `5`, got `number`
// expect-error-count: 4
class Counter {
  n: 5 = 5;
}

type Box = { c: 0 | 1; s: "" };

function main(): void {
  const box: Box = { c: 1, s: "" };
  box.c++;
  box.s += "z";
  const zeros: 0[] = [0];
  zeros[0] += 2;
  const counter = new Counter();
  counter.n--;
}
