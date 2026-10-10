interface Base { [key: string]: number | string; }
interface Child extends Base { [key: string]: number; }
function main(): void { const c: Child = { x: 1 }; assert(c.x === 1); }
