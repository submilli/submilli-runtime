// @target:es5, es2015

let comp: number = null as unknown as (number);

comp **= 1;
comp **= comp **= 1;
comp **= comp **= 1 + 2;
comp **= comp **= 1 - 2;
comp **= comp **= 1 * 2;
comp **= comp **= 1 / 2;

comp **= comp **= (1 + 2);
comp **= comp **= (1 - 2);
comp **= comp **= (1 * 2);
comp **= comp **= (1 / 2);

comp **= comp **= 1 + 2 ** 3;
comp **= comp **= 1 - 2 ** 4;
comp **= comp **= 1 * 2 ** 5;
comp **= comp **= 1 / 2 ** 6;

comp **= comp **= (1 + 2) ** 3;
comp **= comp **= (1 - 2) ** 4;
comp **= comp **= (1 * 2) ** 5;
comp **= comp **= (1 / 2) ** 6;


function main(): void {}
