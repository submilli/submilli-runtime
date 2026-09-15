export class Account {
  private balance: number = 100;

  withdraw(n: number): number {
    this.balance -= n;
    return this.balance;
  }

  invariantHolds(): boolean {
    return this.balance >= 0;
  }
}
