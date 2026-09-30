// @target: es2015
// Derived member is optional but base member is not, should be an error

interface Base { foo: string; }
interface Derived extends Base { bar: string; }

interface T {
    Foo: Base;
}

interface S extends T {
    Foo?: Derived // error
}

/*pruned*/;   
            
 

/*pruned*/;              
                         
 

interface T3 {
    '1': Base;
}

interface S3 extends T3 {
    '1'?: Derived; // error
}

// object literal case
let a: { Foo: Base; } = null as unknown as ({ Foo: Base; });
let b: { Foo?: Derived; } = null as unknown as ({ Foo?: Derived; });
let r = true ? a : b; // ok

function main(): void {}
