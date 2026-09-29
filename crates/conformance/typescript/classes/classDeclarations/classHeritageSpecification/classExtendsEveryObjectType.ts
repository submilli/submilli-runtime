// @target: es2015
interface I {
    foo: string;
}
class C extends I { } // error

class C2 extends { foo: string; } { } // error
let x: { foo: string; } = null as unknown as ({ foo: string; });
class C3 extends x { } // error

/*pruned*/;                      
/*pruned*/;            // error

function foo(): void { }
class C5 extends foo { } // error

/*pruned*/;            // error

function main(): void {}
