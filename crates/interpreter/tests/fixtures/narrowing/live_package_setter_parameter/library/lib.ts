/** Store values. */
export class Box {
 private saved: number = 3;
 set value(next: number) { this.saved = next; }
 read(): number { return this.saved; }
}
