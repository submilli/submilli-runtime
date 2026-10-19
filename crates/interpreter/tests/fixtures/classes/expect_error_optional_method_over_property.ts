// expect-error: `f` redeclares an inherited property as an optional method
// A bodyless optional method can't stand in for an inherited property, or a
// subclass method could then override the property through it.
class Base {
  f?: () => number = (): number => 5;
}
class Mid extends Base {
  f?(): number;
}
class Low extends Mid {
  f(): number {
    return 2;
  }
}
function main(): void {}
