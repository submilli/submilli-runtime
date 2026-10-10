// expect-error: expected `string`, got `string | null`

class Flip {
  private reads: number = 0;

  get value(): string | null {
    this.reads = this.reads + 1;
    return this.reads === 1 ? "first" : null;
  }
}

function read(value: Flip): string {
  if (value.value !== null) {
    return value.value;
  }
  return "none";
}

export function main(): void {
  read(new Flip());
}
