// @target: es5, es2015
// @lib: es5,es2017.object

let o = { a: 1, b: 2 };

for (let x of Object.values(o)) {
    let y = x;
}

let entries = Object.entries(o);                    // [string, number][]
let values = Object.values(o);                      // number[]

let entries1 = Object.entries(1);                   // [string, any][]
let values1 = Object.values(1);                     // any[]

let entries2 = Object.entries({ a: true, b: 2 });   // [string, number|boolean][]
let values2 = Object.values({ a: true, b: 2 });     // (number|boolean)[]

let entries3 = Object.entries({});                  // [string, {}][]
let values3 = Object.values({});                    // {}[]

let a = ["a", "b", "c"];
let entries4 = Object.entries(a);                   // [string, string][]
let values4 = Object.values(a);                     // string[]

enum E { A, B }
/*pruned*/;                                         // [string, any][]
/*pruned*/;                                         // any[]

interface I { }
let i: I = {};
let entries6 = Object.entries(i);                   // [string, any][]
let values6 = Object.values(i);                     // any[]

function main(): void {}
