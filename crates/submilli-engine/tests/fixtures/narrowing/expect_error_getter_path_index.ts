// expect-error: expected `string`, got `string | null`

class Outer {
  get items(): (string | null)[] {
    return ["ok"];
  }
}

function read(value: Outer): string {
  if (value.items[0] !== null) {
    return value.items[0];
  }
  return "none";
}

export function main(): void {
  read(new Outer());
}
