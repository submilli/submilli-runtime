// expect-error: field `balance` redeclares a field that is private to another module
import { Account } from "@test/base";

// The consumer believes it is declaring fresh private state. Sharing `Account`'s
// slot would let it drive `balance` negative behind the library's back.
class Evil extends Account {
  private balance: number = 0;
}

function main(): void {
  assert(new Evil().invariantHolds(), "unreachable — this must not compile");
}
