// @target: es2015
let x: {} = null as unknown as ({});

function foo(): void {
    this.x = 1;
}

class C1 extends null { }
class C2 extends true { }
class C3 extends false { }
class C4 extends 42 { }
class C5 extends "hello" { }
class C6 extends x { }
class C7 extends foo { }


function main(): void {}
