// SlackError. `src/lib.ts` re-exports it; the metadata parser throws it too.

/** A Slack transport or application error with a stable Slack error code. */
export class SlackError extends Error {
    code: string;
    status: number;

    constructor(code: string, message: string, status: number) {
        super(message);
        this.name = "SlackError";
        this.code = code;
        this.status = status;
    }
}
