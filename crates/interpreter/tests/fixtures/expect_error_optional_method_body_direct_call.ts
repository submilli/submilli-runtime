// expect-error: undefined
class Receiver { read?(): number { return 3; } }
function main(): void { new Receiver().read(); }
