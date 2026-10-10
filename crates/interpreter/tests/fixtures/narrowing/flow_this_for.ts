class C {
 value: string | null = "a";
 run(): string { let s = ""; let i=0; if(this.value !== null) { for(;i<3;i+=1) { s = s + this.value; this.value="b"; } } return s; }
}
function main(): void { assert(new C().run() === "abb", "this"); }
