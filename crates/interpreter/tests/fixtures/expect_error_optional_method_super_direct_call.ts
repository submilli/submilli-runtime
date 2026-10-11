// expect-error: undefined
class Base { read?(): number { return 3; } }
class Child extends Base { invoke(): number { return super.read(); } }
function main(): void { new Child().invoke(); }
