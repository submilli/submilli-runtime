function nothing(): void {}
function main(): void {
  let matched = false;
  switch (nothing()) {
    case undefined: matched = true; break;
  }
  assert(matched, "void switch discriminant matches undefined");
}
