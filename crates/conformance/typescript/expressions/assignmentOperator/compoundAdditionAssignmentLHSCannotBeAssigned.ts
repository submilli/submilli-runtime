// @target: es2015
// string can add every type, and result string cannot be assigned to below types
enum E { a, b, c }

let x1: boolean = null as unknown as (boolean);
x1 += '';

let x2: number = null as unknown as (number);
x2 += '';

/*pruned*/;                        
/**/;    

let x4: {a: string} = null as unknown as ({a: string});
x4 += '';

let x5: void = null as unknown as (void);
x5 += '';

function main(): void {}
