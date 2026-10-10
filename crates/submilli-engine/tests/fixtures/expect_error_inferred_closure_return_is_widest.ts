// expect-error-count: 1
// expect-error: wings
// The closure returns its widest `return`, so a member only the narrower one
// has is not readable on the result.

class Animal {
  legs: number = 4;
}
class Bird extends Animal {
  wings: number = 2;
}

function main(): void {
  const key: string = ["a"][0];
  const pick = () => {
    if (key === "b") {
      return new Bird();
    }
    return new Animal();
  };
  console.log(pick().wings);
}
