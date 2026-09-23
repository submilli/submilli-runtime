function invoke(cb: () => void): void { cb(); }
function main(): void {
 const dst: number[] = [];
 [1, 2, 3].forEach(v => dst.push(v));
 invoke(() => { return dst.push(4); });
 const push = (): number => dst.push(5);
 invoke(push);
 const discard: () => void = push;
 discard();
 assert(dst.join(",") === "1,2,3,4,5,5", "callbacks preserve effects");
}
