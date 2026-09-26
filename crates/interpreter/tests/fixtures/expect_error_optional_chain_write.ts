// The arguments after a `?.` are skipped when the receiver is null, so a
// write in them may not have happened (TS18047).
// expect-error: cannot read field `length` on `string | null`

class Measure {
  of(n: number): number {
    return n;
  }
}

function main(): void {
  let x: string | null = null as string | null;
  const m: Measure | null = null as Measure | null;
  m?.of((x = "hello").length);
  console.log(`${x.length}`);
}
