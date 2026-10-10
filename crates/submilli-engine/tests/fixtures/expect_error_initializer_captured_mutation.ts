// expect-error: receiver can be `null`
function main(): void {
  let value: string | null = "safe";
  const clear = (): void => { value = null; };
  clear();
  console.log(value.length);
}
