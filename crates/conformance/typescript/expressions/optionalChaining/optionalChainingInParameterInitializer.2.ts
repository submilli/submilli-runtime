// @strict: true
// @target: esnext,es2015,es5
// @noTypesAndSymbols: true
// @noEmit: true

// https://github.com/microsoft/TypeScript/issues/36295
const a = (): { d: string } | undefined => undefined;
((b: string | undefined = a()?.d) => { let a; })();

const x = "";
((b: string | undefined = a()?.d, d: string = x) => { let x; })();

function main(): void {}
