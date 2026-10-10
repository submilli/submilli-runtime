// expect-error-count: 9
// expect-error: `public` is a reserved word in strict mode and can't be used as a name
// expect-error: `yield` is a reserved word in strict mode and can't be used as a name
// Strict mode reserves these words, so they can't name a binding or a type.
class C {
  constructor(public: number) {}
}
class D {
  constructor(readonly public: number) {}
}
function f(private: number, yield: number): void {}
const static = 1;
function package(): void {}
class protected {}
const { private } = { private: 2 };
type F = (yield: number) => void;
function main(): void {}
