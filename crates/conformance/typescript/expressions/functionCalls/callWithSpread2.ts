// @target: es2015
function all(a?: number, b?: number): void { }
function weird(a?: number | string, b?: number | string): void { }
function prefix(s: string, a?: number, b?: number): void { }
function rest(s: string, a?: number, b?: number,  ...rest: number[]): void { }
function normal(s: string): void { }
function thunk(): string { return null as unknown as (string); }
function prefix2(s: string, n: number, a?: number, b?: number): void { }

let ns: number[] = null as unknown as (number[]);
let mixed: (number | string)[] = null as unknown as ((number | string)[]);
let tuple: [number, string] = null as unknown as ([number, string]);

// good
all(...ns)
weird(...ns)
weird(...mixed)
weird(...tuple)
prefix("a", ...ns)
rest("d", ...ns)


// extra arguments
normal("g", ...ns)
thunk(...ns)

// bad
all(...mixed)
all(...tuple)
prefix("b", ...mixed)
prefix("c", ...tuple)
rest("e", ...mixed)
rest("f", ...tuple)
prefix(...ns) // required parameters are required
prefix(...mixed)
prefix(...tuple)
prefix2("g", ...ns);


function main(): void {}
