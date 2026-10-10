// expect-error: legacy octal literal `010` is not allowed
// expect-error: legacy octal literal `00` is not allowed
// expect-error: legacy octal literal `0777` is not allowed
// expect-error: legacy octal literal `010n` is not allowed
// expect-error-count: 6
function main(): void {
  console.log(010);
  console.log(00);
  console.log(-0777);
  console.log(010n);
  console.log(010.toString());
  const octal: number = 07;
  console.log(octal);
}
