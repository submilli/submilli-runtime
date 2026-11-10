// @target: es5, es2015

let t1 = 10;
let t2 = 10;
let s;

// TempateHead & TemplateTail are empty
`${t1 ** t2}`;
`${t1 ** t2 ** t1}`;
`${t1 + t2 ** t1}`;
`${t1 ** t2 + t1}`;
`${t1 + t2 ** t2 + t1 }`;
`${typeof (t1 ** t2 ** t1) }`;
`${1 + typeof (t1 ** t2 ** t1) }`;

`${t1 ** t2}${t1 ** t2}`;
`${t1 ** t2 ** t1}${t1 ** t2 ** t1}`;
`${t1 + t2 ** t1}${t1 + t2 ** t1}`;
`${t1 ** t2 + t1}${t1 ** t2 + t1}`;
`${t1 + t2 ** t2 + t1}${t1 + t2 ** t2 + t1}`;
`${typeof (t1 ** t2 ** t1)}${typeof (t1 ** t2 ** t1)}`;

`${t1 ** t2} hello world ${t1 ** t2}`;
`${t1 ** t2 ** t1} hello world ${t1 ** t2 ** t1}`;
`${t1 + t2 ** t1} hello world ${t1 + t2 ** t1}`;
`${t1 ** t2 + t1} hello world ${t1 ** t2 + t1}`;
`${t1 + t2 ** t2 + t1} hello world ${t1 + t2 ** t2 + t1}`;
`${typeof (t1 ** t2 ** t1) } hello world ${typeof (t1 ** t2 ** t1) }`;

function main(): void {}
