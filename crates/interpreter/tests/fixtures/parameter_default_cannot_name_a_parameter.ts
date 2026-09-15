// A default is evaluated in the function's own scope, so an identifier naming a
// parameter refers to that parameter — including when it shadows a prelude
// global or an enum. Saying "not yet supported" would be wrong: spec.md makes
// this a permanent rule.
//
// One parameter name per arm that can meet an identifier, so each needle below
// pins exactly one: a plain reference, a global in the `Identifier` arm, a
// global under negation, and an enum in the `FieldAccess` arm. All four must
// reach the same message.
// expect-error: a default value cannot reference the parameter `a`
// expect-error: a default value cannot reference the parameter `Infinity`
// expect-error: a default value cannot reference the parameter `NaN`
// expect-error: a default value cannot reference the parameter `E`

enum E { A = 1, B = 2 }

function plain(a: number, x: number = a): number { return x; }
function shadowsGlobal(Infinity: number, x: number = Infinity): number { return x; }
function shadowsNegatedGlobal(NaN: number, x: number = -NaN): number { return x; }
function shadowsEnum(E: E, x: E = E.B): boolean { return x === E; }

export function main(): string { return "x"; }
