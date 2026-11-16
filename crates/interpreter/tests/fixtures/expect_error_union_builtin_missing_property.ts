// expect-error: cannot read field `byteLength`
function missing(value: string[] | Uint8Array): number {
  return value.byteLength;
}
function main(): void {}
