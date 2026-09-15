// test262: test/built-ins/Set/prototype/add/preserves-insertion-order.js
// Adapted: expects.shift() rewritten as index access (no Array#shift);
// Set#forEach's callback takes the value only.

function main(): void {
  const s = new Set<number>();
  const expects: number[] = [1, 2, 3];

  s.add(1).add(2).add(3);

  let i = 0;
  s.forEach((value: number): void => {
    assertSameValue(value, expects[i]);
    i++;
  });

  assertSameValue(expects.length - i, 0, "The value of `expects.length` is `0`");
}
