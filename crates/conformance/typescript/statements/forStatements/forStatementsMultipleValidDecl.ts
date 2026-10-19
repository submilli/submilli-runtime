// @target: es2015
// @allowUnreachableCode: true

// all expected to be valid

for (let x: number; ;) { }
for (let x = 2; ;) { }

for (let x = <number>undefined; ;) { }
// new declaration space, making redeclaring x as a string valid
function declSpace(): void {
    for (let x = 'this is a string'; ;) { }
}
interface Point { x: number; y: number; }

for (let p: Point; ;) { }
for (let p = { x: 1, y: 2 }; ;) { }
for (let p: Point = { x: 0, y: undefined }; ;) { }
for (let p = { x: 1, y: <number>undefined }; ;) { }
for (let p: { x: number; y: number; } = { x: 1, y: 2 }; ;) { }
for (let p = <{ x: number; y: number; }>{ x: 0, y: undefined }; ;) { }
/*pruned*/;                 

for (let fn = function (s: string) { return 42; }; ;) { }
for (let fn = (s: string) => 3; ;) { }
for (let fn: (s: string) => number; ;) { }
/*pruned*/;                                 
for (let fn = <(s: string) => number> null; ;) { }
/*pruned*/;                   

for (let a: string[]; ;) { }
for (let a = ['a', 'b']; ;) { }
for (let a = <string[]>[]; ;) { }
for (let a: string[] = []; ;) { }
/*pruned*/;                             
/*pruned*/;                 

function main(): void {}
