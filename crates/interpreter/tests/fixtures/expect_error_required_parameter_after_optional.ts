// A required parameter cannot follow an optional one in any parameter list:
// a function, an arrow, or a function type. Each reports the same error.
// expect-error: required parameter `b` cannot follow an optional parameter
// expect-error: required parameter `d` cannot follow an optional parameter
// expect-error: required parameter `f` cannot follow an optional parameter
// expect-error: make it optional too (`x?: T`), give it a default (`x: T = …`), or move it before the optional parameters
// expect-error-count: 3
function declared(a?: number, b: number): void {}

const arrow = (c?: number, d: number): void => {};

type Callback = (e?: number, f: number) => void;

function main(): void {}
