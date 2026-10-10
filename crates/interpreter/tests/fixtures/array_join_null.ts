// A `null` element behaves as in JavaScript: it joins as the empty string, sorts
// as the string "null", and is found by a `null` needle.
class Point {
  constructor(public x: number) {}
}

function main(): void {
  const xs: (number | null)[] = [1, null, 3];
  assert(xs.join("-") === "1--3", "join with a separator");
  assert(xs.join() === "1,,3", "join with the default separator");
  assert(xs.toString() === "1,,3", "toString");
  assert(`${xs}` === "1,,3", "template interpolation");
  const pair: [number, string | null] = [1, null];
  assert(pair.join() === "1,", "a tuple element");
  console.log(xs);

  assert(xs.indexOf(null) === 1, "indexOf finds a null element");
  assert(xs.lastIndexOf(null) === 1, "lastIndexOf finds a null element");
  assert(xs.includes(null), "includes finds a null element");
  assert([1, 3].indexOf(1) === 0, "non-null needles are unchanged");
  const points: (Point | null)[] = [new Point(1), null];
  assert(points.indexOf(null) === 1, "a null needle skips class elements");
  const noNulls: (Point | null)[] = [new Point(1)];
  assert(noNulls.indexOf(null) === -1, "and finds nothing without a null");

  const words: (string | null)[] = ["o", null, "a"];
  words.sort();
  assert(words.join() === "a,,o", "null sorts as the string \"null\"");
}
