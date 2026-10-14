// Only a `typeof` value may leave tags out of a switch without `default`; a
// declared union that happens to spell tags must list every member.
// expect-error: "object"
// expect-error-count: 1
function kind(x: "string" | "object"): string {
  switch (x) {
    case "string":
      return "s";
  }
  return "other";
}

function main(): void {
  console.log(kind("string"));
}
