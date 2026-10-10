// A `do … while` condition runs before the next pass through the body, so a
// write in it undoes a narrowing the body relies on (TS18047).
// expect-error: cannot read field `length` on `string | null`

function main(): void {
  let x: string | null = "a";
  let n = 0;
  if (x !== null) {
    do {
      console.log(`${x.length}`);
      n++;
    } while ((x = null) === null && n < 2);
  }
}
