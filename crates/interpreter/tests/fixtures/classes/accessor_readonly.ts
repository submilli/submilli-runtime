// expect-error: cannot assign to read-only accessor `value`
class Box {
  private stored: number = 7;
  get value(): number {
    return this.stored;
  }
}

function main(): void {
  const b = new Box();
  // A get-only accessor is read-only; assigning is a compile error.
  b.value = 9;
}
