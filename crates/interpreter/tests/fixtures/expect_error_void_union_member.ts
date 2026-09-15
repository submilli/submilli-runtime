// expect-error: `void` cannot be a union member — it has no values
function unionMember(): void | null {
  return null;
}

function main(): void {}
