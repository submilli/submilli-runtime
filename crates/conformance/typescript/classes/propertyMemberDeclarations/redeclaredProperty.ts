// @noTypesAndSymbols: true
// @strictNullChecks: true
// @target: esnext
// @useDefineForClassFields: true
class Base {
  b: number = 1;
}

class Derived extends Base {
  b: number;
  d: number = this.b;

  constructor() {
    super();
    this.b = 2;
  }
}


function main(): void {}
