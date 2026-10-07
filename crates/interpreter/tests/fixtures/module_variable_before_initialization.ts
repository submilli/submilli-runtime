// A function that runs during module initialization and reaches a module
// variable or static field whose declaration hasn't run yet throws a
// `ReferenceError`, as in JavaScript. After the declaration runs, the same
// function reads the initialized value.
function readNumber(): number { return laterNumber; }
function readString(): string { return laterString; }
function readObject(): { a: number } { return laterObject; }
function writeNumber(): void { laterNumber = 3; }
function bumpNumber(): void { laterNumber++; }
function readStatic(): number { return Later.value; }

function failure(run: () => void): string {
  try {
    run();
    return "none";
  } catch (e) {
    return e instanceof ReferenceError ? `${e.name}: ${e.message}` : "other";
  }
}

const early: string[] = [
  failure(() => { readNumber(); }),
  failure(() => { readString(); }),
  failure(() => { readObject(); }),
  failure(writeNumber),
  failure(bumpNumber),
  failure(() => { readStatic(); }),
];

let laterNumber = 5;
const laterString = "s";
const laterObject = { a: 1 };
class Later {
  static value: number = 7;
}

// A class is initialized before its static initializers run, so they can read
// its later statics, which still hold their defaults.
class SelfInit {
  static first: number = 1;
  static sum: number = SelfInit.add();
  static add(): number { return SelfInit.first + SelfInit.later; }
  static later: number = 5;
}

function main(): void {
  const number = "ReferenceError: Cannot access 'laterNumber' before initialization";
  assert(early[0] === number, "a number read");
  assert(early[1] === "ReferenceError: Cannot access 'laterString' before initialization", "a string read");
  assert(early[2] === "ReferenceError: Cannot access 'laterObject' before initialization", "an object read");
  assert(early[3] === number, "a write");
  assert(early[4] === number, "an increment");
  assert(early[5] === "ReferenceError: Cannot access 'Later' before initialization", "a static field read");
  assert(laterNumber === 5, "the failed write left the initializer's value");
  assert(readNumber() === 5 && readString() === "s" && readObject().a === 1, "reads after initialization");
  assert(readStatic() === 7, "a static read after initialization");
  assert(SelfInit.sum === 1, "a static initializer reads a later static's default");
  writeNumber();
  bumpNumber();
  assert(laterNumber === 4, "writes after initialization");
}
