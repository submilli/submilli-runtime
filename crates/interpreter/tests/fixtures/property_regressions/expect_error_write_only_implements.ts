// expect-error: member `note` is write-only (no getter)
interface Bag { note: string; }
class WriteOnly implements Bag {
    set note(value: string) {}
}
function main(): void {}
