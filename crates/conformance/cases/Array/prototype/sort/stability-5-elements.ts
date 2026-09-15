// test262: test/built-ins/Array/prototype/sort/stability-5-elements.js

interface Entry {
  name: string;
  rating: number;
}

function main(): void {
  const array: Entry[] = [
    { name: "A", rating: 2 },
    { name: "B", rating: 3 },
    { name: "C", rating: 2 },
    { name: "D", rating: 3 },
    { name: "E", rating: 3 },
  ];
  assertSameValue(array.length, 5);

  // Sort the elements by `rating` in descending order.
  // (This updates `array` in place.)
  array.sort((a: Entry, b: Entry): number => b.rating - a.rating);

  const reduced = array.reduce((acc: string, element: Entry): string => acc + element.name, "");
  assertSameValue(reduced, "BDEAC");
}
