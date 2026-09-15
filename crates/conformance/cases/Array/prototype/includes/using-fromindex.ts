// test262: test/built-ins/Array/prototype/includes/using-fromindex.js

function main(): void {
  const sample: string[] = ["a", "b", "c"];
  assertSameValue(sample.includes("a", 0), true, "includes('a', 0)");
  assertSameValue(sample.includes("a", 1), false, "includes('a', 1)");
  assertSameValue(sample.includes("a", 2), false, "includes('a', 2)");

  assertSameValue(sample.includes("b", 0), true, "includes('b', 0)");
  assertSameValue(sample.includes("b", 1), true, "includes('b', 1)");
  assertSameValue(sample.includes("b", 2), false, "includes('b', 2)");

  assertSameValue(sample.includes("c", 0), true, "includes('c', 0)");
  assertSameValue(sample.includes("c", 1), true, "includes('c', 1)");
  assertSameValue(sample.includes("c", 2), true, "includes('c', 2)");

  assertSameValue(sample.includes("a", -1), false, "includes('a', -1)");
  assertSameValue(sample.includes("a", -2), false, "includes('a', -2)");
  assertSameValue(sample.includes("a", -3), true, "includes('a', -3)");
  assertSameValue(sample.includes("a", -4), true, "includes('a', -4)");

  assertSameValue(sample.includes("b", -1), false, "includes('b', -1)");
  assertSameValue(sample.includes("b", -2), true, "includes('b', -2)");
  assertSameValue(sample.includes("b", -3), true, "includes('b', -3)");
  assertSameValue(sample.includes("b", -4), true, "includes('b', -4)");

  assertSameValue(sample.includes("c", -1), true, "includes('c', -1)");
  assertSameValue(sample.includes("c", -2), true, "includes('c', -2)");
  assertSameValue(sample.includes("c", -3), true, "includes('c', -3)");
  assertSameValue(sample.includes("c", -4), true, "includes('c', -4)");
}
