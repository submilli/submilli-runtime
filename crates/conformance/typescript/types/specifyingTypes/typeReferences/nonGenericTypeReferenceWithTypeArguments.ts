// @target: es2015
// Check that errors are reported for non-generic types with type arguments

class C { }
interface I { }
enum E { }
type T = { };
let v1: C<string> = null as unknown as (C<string>);
let v2: I<string> = null as unknown as (I<string>);
let v3: E<string> = null as unknown as (E<string>);
let v4: T<string> = null as unknown as (T<string>);

function f<U>(): void {
    class C { }
    interface I { }
    enum E { }
    type T = {};
    let v1: C<string> = null as unknown as (C<string>);
    let v2: I<string> = null as unknown as (I<string>);
    let v3: E<string> = null as unknown as (E<string>);
    let v4: T<string> = null as unknown as (T<string>);
    let v5: U<string> = null as unknown as (U<string>);
}


function main(): void {}
