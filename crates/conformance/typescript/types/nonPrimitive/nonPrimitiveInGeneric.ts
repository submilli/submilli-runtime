// @target: es2015
// @declaration: true
function generic<T>(t: T): void {
    let o: object = t; // expect error
}
let a = {};
let b = "42";

generic<object>({});
generic<object>(a);
generic<object>(123); // expect error
generic<object>(b); // expect error

function bound<T extends object>(t: T): void {
    let o: object = t; // ok
}

bound({});
bound(a);
bound(123); // expect error
bound(b); // expect error

function bound2<T extends object>(): void {}

bound2<{}>();
bound2<Object>();
bound2<number>(); // expect error
bound2<string>(); // expect error

function bound3<T extends {}>(t: T): void {
    let o: object = t; // ok
}

interface Proxy<T extends object> {}

let x: Proxy<number> = null as unknown as (Proxy<number>); // error
let y: Proxy<null> = null as unknown as (Proxy<null>); // ok
let z: Proxy<undefined> = null as unknown as (Proxy<undefined>) ; // ok


interface Blah {
    foo: number;
}

let u: Proxy<Blah> = null as unknown as (Proxy<Blah>); // ok


function main(): void {}
