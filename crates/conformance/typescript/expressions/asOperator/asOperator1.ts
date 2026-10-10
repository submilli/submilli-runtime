// @target: es2015
let as = 43;
let x = undefined as number;
let y = (null as string).length;
/*pruned*/;                   

// Should parse as a union type, not a bitwise 'or' of (32 as number) and 'string'
let j = 32 as number|string;
j = '';


function main(): void {}
