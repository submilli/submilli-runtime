// @target: es2015
// Base property is optional and derived type has no property of that name

interface Base { foo: string; }
interface Derived extends Base { bar: string; }

interface T {
    Foo?: Base;
}

interface S extends T {
    Foo2: Derived // ok
}

/*pruned*/;   
             
 

/*pruned*/;              
                     
 

interface T3 {
    '1'?: Base;
}

interface S3 extends T3 {
    '1.0': Derived; // ok
}

// object literal case
let a: { Foo?: Base; } = null as unknown as ({ Foo?: Base; });
let b: { Foo2: Derived; } = null as unknown as ({ Foo2: Derived; });
let r = true ? a : b; // ok

function main(): void {}
