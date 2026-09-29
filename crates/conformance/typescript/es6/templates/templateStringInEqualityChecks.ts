// @target: es2015
let x = `abc${0}abc` === `abc` ||
        `abc` !== `abc${0}abc` &&
        `abc${0}abc` == "abc0abc" &&
        "abc0abc" !== `abc${0}abc`;

function main(): void {}
