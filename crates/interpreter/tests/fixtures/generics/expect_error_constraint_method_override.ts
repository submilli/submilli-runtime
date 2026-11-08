// expect-error: incompatible inherited member
interface Parent { pick<T>(value: T): T; }
interface Child extends Parent { pick<T extends string>(value: T): T; }
function main(): void {}
