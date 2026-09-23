// expect-error: field `lenght` does not exist on `number[]`
// expect-error: field `lenght` does not exist on `string`
// expect-error: did you mean `length`?
function main(): void {
  const a: number[] = [1];
  const b: number[] | null = a;
  const s: string = "abc";
  const t: string | null = s;
  console.log(a.lenght);
  console.log(b?.lenght);
  console.log(s.lenght);
  console.log(t?.lenght);
}
