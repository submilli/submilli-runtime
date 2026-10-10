// expect-error: field `balance` redeclares an inherited field at a different visibility
// expect-error: field `token` redeclares an inherited field at a different visibility
// The redeclaration shares the inherited field's one storage slot, so a
// visibility mismatch aliases the parent's storage through the wrong access
// level — in either direction.

class Account {
  private balance: number = 100;
  withdraw(n: number): number {
    this.balance -= n;
    return this.balance;
  }
}

// `public` over `private`: the parent's encapsulated state becomes writable by
// anyone holding the subclass.
class Tagged extends Account {
  balance: number = 0;
}

class Session {
  token: string = "public";
}

// `private` over `public`: the field stays reachable through a `Session`-typed
// reference regardless, so the `private` is not enforceable.
class Locked extends Session {
  private token: string = "secret";
}

function main(): void {}
