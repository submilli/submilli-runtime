// @target: es2015
// @strict: true

// Repro from #20840

function function1<T extends 'a' | 'b'>(key: T): void {
  switch (key) {
    case 'a':
      key.toLowerCase();
      break;
    default:
      key.toLowerCase();
      break;
  }
}


function main(): void {}
