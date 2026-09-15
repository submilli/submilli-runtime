// When the signature pass skips an accessor — here because the name clashes
// with a field — the body pass is the only one to resolve its annotation, so
// its diagnostics are the originals, not replays, and must survive.
// expect-error: duplicate member `a` on class `Holder`
// expect-error: unknown type `Bogus`
class Holder {
  a: number = 0;
  set a(x: Bogus) {}
}

function main(): void {}
