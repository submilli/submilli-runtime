// A function literal without type parameters gets no contextual parameter
// types from a generic function type: its parameters would name type
// parameters it doesn't have (tsc makes the literal generic).
// expect-error: parameter `x` requires a type annotation
// expect-error-count: 1
function main(): void {
  const f: <X>(x: X) => X = (x) => x;
}
