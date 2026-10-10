// expect-error: expected `string`, got `string | null`

class Inner {
  name: string | null = "ok";
}

class Outer {
  get inner(): Inner {
    return new Inner();
  }
}

function read(value: Outer): string {
  if (value.inner.name !== null) {
    return value.inner.name;
  }
  return "none";
}

export function main(): void {
  read(new Outer());
}
