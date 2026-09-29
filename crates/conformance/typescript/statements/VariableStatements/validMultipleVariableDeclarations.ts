// @target: es2015
// all expected to be valid

let x: number = null as unknown as (number);
let x_2 = 2;
if (true) {
    let x = 3;
    for (let x = 0; ;) { }
}
let x_3 = <number>null;

// new declaration space, making redeclaring x as a string valid
function declSpace(): void {
    let x = 'this is a string';
}

interface Point { x: number; y: number; }

let p: Point = null as unknown as (Point);
let p_2 = { x: 1, y: 2 };
let p_3: Point = { x: 0, y: null };
let p_4 = { x: 1, y: <number>null };
let p_5: { x: number; y: number; } = { x: 1, y: 2 };
let p_6 = <{ x: number; y: number; }>{ x: 0, y: null };
let p_7: typeof p = null as unknown as (typeof p);

let fn = function (s: string) { return 42; }
let fn_2 = (s: string) => 3;
let fn_3: (s: string) => number = null as unknown as ((s: string) => number);
/*pruned*/;                                                                      
let fn_5 = <(s: string) => number> null;
let fn_6: typeof fn = null as unknown as (typeof fn);

let a: string[] = null as unknown as (string[]); 
let a_2 = ['a', 'b']
let a_3 = <string[]>[];
let a_4: string[] = [];
/*pruned*/;                   
let a_6: typeof a = null as unknown as (typeof a);


function main(): void {}
