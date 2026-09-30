// @target: es2015
interface I<T, U> {
    tuple1: [T, U];
} 

let i1: I<string, number> = null as unknown as (I<string, number>);
let i1_2: I<string, number> = null as unknown as (I<string, number>);
let i2: I<{}, {}> = null as unknown as (I<{}, {}>);

// no error
i1.tuple1 = ["foo", 5];
let e1 = i1.tuple1[0];  // string
let e2 = i1.tuple1[1];  // number
i1.tuple1 = ["foo", 5, false, true];
let e3 = i1.tuple1[2];  // {}
/*pruned*/;                    
let e4 = i1.tuple1[3];  // {}
i2.tuple1 = ["foo", 5];
i2.tuple1 = ["foo", "bar"];
i2.tuple1 = [5, "bar"];
i2.tuple1 = [{}, {}];

// error
i1.tuple1 = [5, "foo"];
i1.tuple1 = [{}, {}];
i2.tuple1 = [{}];


function main(): void {}
