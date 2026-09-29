// @target: es2015
let regex1 = / asdf /;
let regex2 = /**// asdf /;
let regex3 = /**///**/ asdf /       // should be a comment line
1;
let regex4 = /**// /**/asdf /;
let regex5 = /**// asdf/**/ /;

function main(): void {}
