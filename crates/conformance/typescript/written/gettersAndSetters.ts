// Written for Submilli: the upstream accessor cases Submilli can run check nothing
// of what a setter is given, since a value assigned to one isn't read again.

class Temperature {
    private celsius: number = 0;
    get fahrenheit(): number {
        return this.celsius * 9 / 5 + 32;
    }
    set fahrenheit(value: number) {
        this.celsius = (value - 32) * 5 / 9;
    }
    get label(): string {
        return this.celsius > 25 ? "warm" : "cool";
    }
}

let reading = new Temperature();
let set = (reading.fahrenheit = 212);
let raised = (reading.fahrenheit += 18);
let current = reading.fahrenheit;
let text = reading.label;
let summary = `${reading.fahrenheit} F, ${text}`;
reading.label = "hot";
reading.fahrenheit = "hot";

function main(): void {}
