// @noTypesAndSymbols: true
// @strictNullChecks: true
// @target: esnext
// @useDefineForClassFields: true
class Base {
    a: number = 1;
  }
  
  class Derived extends Base {
    b: number = this.a /*undefined*/;
  
    constructor(public a: number) {
        super();
    }
  }
  

function main(): void {}
