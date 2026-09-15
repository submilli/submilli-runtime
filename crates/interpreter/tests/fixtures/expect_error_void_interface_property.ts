// expect-error: `void` cannot be a field type — it has no values
interface HasVoid {
  f: void;
}

function main(): void {}
