// @target: es2015
/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/

/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/

interface I {
    [x: string]: string;
    y: string;
}

let i: I = null as unknown as (I);
let r4 = i['y'];
let r5 = i['a'];
/*pruned*/

let a: {
    [x: string]: string;
    y: string;
} = null as unknown as ({
    [x: string]: string;
    y: string;
});

let r7 = a['y'];
let r8 = a['a'];
/*pruned*/

let b: { [x: string]: string } = { y: '' }

let r10 = b['y'];
let r11 = b['a'];
/*pruned*/


function main(): void {}
