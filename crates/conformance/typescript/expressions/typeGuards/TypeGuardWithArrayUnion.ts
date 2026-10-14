// @target: es2015
class Message {
    value: string;
}

function saySize(message: Message | Message[]): number | undefined {
    if (message instanceof Array) {
        return message.length;  // Should have type Message[] here
    }
}


function main(): void {}
