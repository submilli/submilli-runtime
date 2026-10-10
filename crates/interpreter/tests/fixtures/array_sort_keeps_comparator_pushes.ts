// `sort` writes its sorted snapshot over the first elements and leaves the
// rest alone, as ECMA-262 does: elements the comparator pushes survive, and an
// array the comparator shrinks grows back to the sorted length.
function main(): void {
  const grown: number[] = [5, 3, 1, 4, 2];
  grown.sort((a: number, b: number) => {
    if (grown.length < 8) {
      grown.push(9);
    }
    return a - b;
  });
  assert(grown.join(",") === "1,2,3,4,5,9,9,9", "pushed elements are kept after the sorted ones");

  const shrunk: number[] = [5, 3, 1, 4, 2];
  shrunk.sort((a: number, b: number) => {
    if (shrunk.length > 2) {
      shrunk.pop();
    }
    return a - b;
  });
  assert(shrunk.join(",") === "1,2,3,4,5", "a popped array is written back in full");

  const source: number[] = [3, 1, 2];
  const copy = source.toSorted((a: number, b: number) => {
    source.push(9);
    return a - b;
  });
  assert(copy.join(",") === "1,2,3" && source.length > 3, "toSorted returns only the snapshot");
}
