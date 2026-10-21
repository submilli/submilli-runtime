// A line break before an arrow's `=>` is a syntax error in JavaScript (tsc:
// TS1200). The arrow still parses, so each one is reported once and the rest
// of the file is checked.
// expect-error-count: 4
// expect-error: a line break is not allowed before `=>`
const a = ()
  => 1;
const b = (x: number) /*
  */ => x;
const c = (x: number): number
  => x;
const d = [1].map(x
  => x);

// A break inside the parameters or before the return type is fine.
const e = (x: number
) => x;
const f = (x: number)
  : number => x;

function main(): void {}
