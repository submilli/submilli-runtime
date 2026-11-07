// expect-error: undefined
function main(): void {
  const pick = (x: number) => {
    if (x > 0) return { n: x };
    return { error: "neg" };
  };
  console.log(pick(-1).error.length);
}
