// The last clause of a switch leaves it by running off its end, so what holds
// there joins what holds after the switch, as a `break` would.
// expect-error: expected `string`, got `number | string`
// expect-error: expected `null`, got `number | null`
// expect-error-count: 2
function pick(): number {
  return 2;
}

function maybe(): number | null {
  return 3;
}

function writtenInDefault(): string {
  let value: string | number = "s";
  switch (pick()) {
    case 1: value = "a"; break;
    default: value = 5;
  }
  const text: string = value;
  return text;
}

function narrowedInCase(): null {
  const value = maybe();
  switch (value) {
    case null: break;
    default: console.log("number");
  }
  const none: null = value;
  return none;
}

function main(): void {
  console.log(writtenInDefault(), narrowedInCase());
}
