// expect-error: circular inheritance
class A extends B {}
class B extends A {}

function main(): void {}
