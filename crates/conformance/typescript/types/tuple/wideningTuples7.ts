// @target: es2015
//@strict: true
//@noImplicitAny: true
let foo = function bar() {
    let intermediate: [string] = null as unknown as ([string]);
    return intermediate = [undefined];
};

function main(): void {}
