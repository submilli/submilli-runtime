// expect-error: can only `extends` another class
interface Shape {
  area(): number;
}

class Circle extends Shape {}

function main(): void {}
