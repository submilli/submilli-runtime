// expect-error: undefined
class Receiver { read?(): number { return 3; } }
function call(receiver: Receiver | null): number | undefined { return receiver?.read(); }
function main(): void { call(new Receiver()); }
