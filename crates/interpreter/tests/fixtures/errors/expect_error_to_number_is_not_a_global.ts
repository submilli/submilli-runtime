// SUB-1830: as in TypeScript, there is no `toNumber` global. `Number(s)` and
// `+s` convert a string.
// expect-error: `toNumber`
function main(): void {
  console.log(toNumber("3"));
}
