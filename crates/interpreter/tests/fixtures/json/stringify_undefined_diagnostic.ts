// expect-error: returns `undefined` when its argument is `undefined`
function format(value: string | undefined): string {
  return JSON.stringify(value).slice(0, 400);
}
function main(): void { format(undefined); }
