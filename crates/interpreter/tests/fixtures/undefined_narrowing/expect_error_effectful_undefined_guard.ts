// expect-error: undefined
function read(box: { value: number | undefined }): number {
  if (box.value !== void (box.value = undefined)) {
    return box.value;
  }
  return 0;
}
function main(): void { read({ value: 1 }); }
