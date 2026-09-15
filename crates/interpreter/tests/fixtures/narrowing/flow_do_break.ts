function main(): void {
 let x: string | null = null;
 do { x = "b"; break; } while(false);
 assert(x.toUpperCase() === "B", "do-break exit");
}
