// A parameter inside a *function type annotation* is resolved by the
// `TypeAnnotationKind::Function` arm, not by `resolve_param`. The function's
// own return stays legitimate — only its parameters occupy value slots.
// expect-error: `void` cannot be a parameter type — it has no values
function nothing(): void {}

function main(): void {
  const f: (x: void) => number = (x) => 1;
}
