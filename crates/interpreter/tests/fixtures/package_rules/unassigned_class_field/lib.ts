// expect-error: property `name` has no initializer and is not assigned in the constructor
// expect-error-count: 1
class Holder {
  name: string;
  constructor() {}
}
