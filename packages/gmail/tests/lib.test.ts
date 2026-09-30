import { label } from "submilli:test";
import {
    GmailError,
    EmailInput,
    ReplyInput,
    LabelChanges,
    SearchOptions,
    Header,
    replyRecipients,
    draftRecipients,
    sendEmail,
    createDraft,
} from "@submilli/gmail";

function main(): void {
    label("GmailError preserves stable fields");
    const error = new GmailError("invalidArgument", "bad message", 400);
    assert(error.code === "invalidArgument", "code is preserved");
    assert(error.status === 400, "status is preserved");

    label("public mail inputs cover compose, reply, search, and labels");
    const email: EmailInput = {
        to: ["person@example.com"],
        subject: "Hello",
        text: "Plain body",
        html: "<p>HTML body</p>",
        attachments: [{ path: "/workspace/report.pdf", mimeType: "application/pdf" }],
    };
    const reply: ReplyInput = { messageId: "m1", text: "Thanks", replyAll: true };
    const labels: LabelChanges = { addLabelIds: ["STARRED"], removeLabelIds: ["UNREAD"] };
    const search: SearchOptions = { limit: 20, includeSpamTrash: false };
    assert(email.attachments !== null && email.attachments.length === 1, "VFS attachments are represented");
    assert(reply.replyAll === true, "reply-all is explicit");
    assert(labels.addLabelIds !== null && labels.addLabelIds.length === 1, "label changes are explicit");
    assert(search.limit === 20, "searches are bounded");

    label("a reply goes to the sender's bare address");
    const original: Header[] = [
        { name: "From", value: '"Doe, Dana" <dana@example.com>' },
        { name: "To", value: "me@example.com, Lee Park <lee@example.com>" },
        { name: "Cc", value: "lee@example.com, kim@example.com (Kim), Dana <dana@example.com>, kim@example.com" },
        { name: "Subject", value: "Hello, <not@an.address>" },
    ];
    const direct = replyRecipients(original, false);
    assert(direct.to.join(" ") === "dana@example.com", "a quoted name holding a comma is one sender");
    assert(direct.cc.length === 0, "a plain reply has no Cc");

    label("reply-all adds the original To, and its Cc leaves out the To addresses");
    const all = replyRecipients(original, true);
    assert(all.to.join(" ") === "dana@example.com me@example.com lee@example.com", "To is the sender, then the original To");
    assert(all.cc.join(" ") === "kim@example.com", "Cc holds each remaining address once");

    label("Reply-To replaces From, and header names match in any case");
    const list: Header[] = [
        { name: "from", value: "Dana <dana@example.com>" },
        { name: "reply-to", value: "Team <team@example.com>, dana@example.com" },
        { name: "TO", value: "undisclosed-recipients:;" },
    ];
    assert(replyRecipients(list, false).to.join(" ") === "team@example.com dana@example.com", "Reply-To is used");
    assert(replyRecipients(list, true).to.join(" ") === "team@example.com dana@example.com", "an empty group adds nobody");
    assert(replyRecipients([], true).to.length === 0, "a message without a sender resolves to nobody");

    label("draft recipients are every To, Cc and Bcc address, in that order");
    const draft: Header[] = [
        { name: "Bcc", value: "audit@example.com" },
        { name: "To", value: "Dana <dana@example.com>,\r\n lee@example.com" },
        { name: "Cc", value: "lee@example.com, =?UTF-8?Q?Park=2C_Kim?= <kim@example.com>" },
        { name: "Subject", value: "Hello, <not@an.address>" },
        { name: "bcc", value: "archive@example.com, dana@example.com" },
    ];
    assert(
        draftRecipients(draft).join(" ") === "dana@example.com lee@example.com kim@example.com audit@example.com archive@example.com",
        "repeated headers are all read and each address appears once",
    );
    assert(draftRecipients([{ name: "Subject", value: "No recipients yet" }]).length === 0, "a draft may have no recipients");

    label("a header that is not plainly addresses is refused");
    for (const value of [
        "Dana",
        "Dana <dana@example.com",
        '"Dana <dana@example.com>',
        "Dana (work <dana@example.com>",
        "dana@example.com lee@example.com",
        "Dana <dana@example.com> lee@example.com",
        "lee@example.com <dana@example.com>",
        "team: dana@example.com, lee@example.com;",
        '"dana"@example.com',
        "=?UTF-8?Q?dana?=@example.com",
        "<>",
    ]) {
        assert(headerFailure(value) === "invalid_recipient", "refused: " + value);
    }

    label("recipient entries must be bare addresses, and the error names the field");
    for (const entry of [
        "",
        "   ",
        "dana",
        "@example.com",
        "dana@",
        "dana@example@com",
        "dana park@example.com",
        "dana@example.com, lee@example.com",
        "dana@example.com\r\nBcc: lee@example.com",
        "dana@example.com\n",
        "Dana <dana@example.com>",
        "<dana@example.com>",
        "dana@example.com (Dana)",
        "team:dana@example.com;",
        '"dana"@example.com',
        "=?UTF-8?Q?dana?=@example.com",
    ]) {
        assert(sendFailure(unsendable([entry], [], [])).startsWith("invalid_recipient: recipient field to "), "to refused: " + entry);
        assert(sendFailure(unsendable(["dana@example.com"], [entry], [])).startsWith("invalid_recipient: recipient field cc "), "cc refused: " + entry);
        assert(sendFailure(unsendable(["dana@example.com"], [], [entry])).startsWith("invalid_recipient: recipient field bcc "), "bcc refused: " + entry);
    }
    assert(draftFailure(unsendable(["dana@example.com"], ["Lee <lee@example.com>"], [])).startsWith("invalid_recipient: recipient field cc "), "drafts refuse the same entries");

    label("an address holds printable ASCII only");
    for (const entry of [
        "blocked@example\uff0ecom",
        "blocked\u200b@example.com",
        "blocked@exam\u00adple.com",
        "blocked@\ufeffexample.com",
        // A string literal cannot hold a lone surrogate.
        "blocked" + String.fromCharCode(0xd800) + "@example.com",
        "blocked@example.com" + String.fromCharCode(0xdc00),
        "bl\u00f6cked@example.com",
        "blocked@example.com\u0000",
        "blocked\u007f@example.com",
    ]) {
        assert(sendFailure(unsendable([entry], [], [])).startsWith("invalid_recipient: recipient field to "), "to refused: " + escaped(entry));
        assert(draftFailure(unsendable(["dana@example.com"], [entry], [])).startsWith("invalid_recipient: recipient field cc "), "cc refused: " + escaped(entry));
        assert(headerFailure(entry) === "invalid_recipient", "header refused: " + escaped(entry));
        assert(headerFailure("Dana <" + entry + ">") === "invalid_recipient", "named header refused: " + escaped(entry));
    }

    label("every ASCII character is accepted or refused by its code unit");
    for (let unit = 0; unit < 128; unit += 1) {
        const entry = "da" + String.fromCharCode(unit) + "na@example.com";
        const accepted = sendFailure(unsendable([entry], [], [])).startsWith("invalid_header: ");
        assert(accepted === isAddressUnit(unit), "code unit " + unit.toString());
    }

    label("whitespace other than space and tab is refused, around an address as inside it");
    for (const unit of [0x00a0, 0x2028, 0x2029, 0x0085, 0x000b, 0x000c, 0x000a, 0x000d]) {
        const mark = String.fromCharCode(unit);
        const followedBySpace = "dana@example.com" + mark + " ";
        assert(sendFailure(unsendable([followedBySpace], [], [])).startsWith("invalid_recipient: recipient field to "), "to refused: " + escaped(followedBySpace));
        assert(headerFailure(followedBySpace) === "invalid_recipient", "header refused: " + escaped(followedBySpace));
        // In a header, a line break followed by a space is folding; a test below covers it.
        for (const entry of ["dana@example.com" + mark, mark + "dana@example.com", "dana@example.com " + mark]) {
            assert(sendFailure(unsendable([entry], [], [])).startsWith("invalid_recipient: recipient field to "), "to refused: " + escaped(entry));
            assert(draftFailure(unsendable(["dana@example.com"], [], [entry])).startsWith("invalid_recipient: recipient field bcc "), "bcc refused: " + escaped(entry));
            assert(headerFailure(entry) === "invalid_recipient", "header refused: " + escaped(entry));
            assert(headerFailure("Dana <" + entry + ">") === "invalid_recipient", "named header refused: " + escaped(entry));
            assert(replyFailure(entry) === "invalid_recipient", "sender refused: " + escaped(entry));
        }
        assert(headerFailure("Dana <dana@example.com>" + mark) === "invalid_recipient", "text after the address refused: " + escaped(mark));
    }
    assert(headerFailure("dana@example.com\r\n") === "invalid_recipient", "a header ending in a line break is refused");
    assert(headerFailure("dana@example.com\r\nBcc: lee@example.com") === "invalid_recipient", "a header holding a second header is refused");

    label("a line break that is not folding is refused wherever it sits");
    for (const value of [
        '"Dana\r\nBcc: victim@evil.com\r\nX-A: " <a@example.com>',
        "a@example.com (note\r\nBcc: victim@evil.com\r\nX-A: x)",
        '"Dana\nBcc: victim@evil.com" <a@example.com>',
        "a@example.com,\n\r\n b@example.com",
    ]) {
        assert(headerFailure(value) === "invalid_recipient", "header refused: " + escaped(value));
        assert(replyFailure(value) === "invalid_recipient", "sender refused: " + escaped(value));
    }
    for (const value of ["a@example.com\r\n\r\n ", "a@example.com,\r\r\n b@example.com", "a@example.com,\r\n \nb@example.com", "a@example.com,\r b@example.com"]) {
        assert(headerFailure(value) === "invalid_recipient", "header refused: " + escaped(value));
    }

    label("a folded quoted name and a folded list still parse");
    const foldedName = '"Dana\r\n Smith" <a@example.com>';
    assert(draftRecipients([{ name: "To", value: foldedName }]).join(" ") === "a@example.com", "a folded name in a draft");
    assert(replyRecipients([{ name: "From", value: foldedName }], false).to.join(" ") === "a@example.com", "a folded name in a reply");
    for (const foldedList of ["a@example.com,\r\n b@example.com", "a@example.com,\n\tb@example.com", "a@example.com,\r\n \r\n b@example.com"]) {
        assert(draftRecipients([{ name: "To", value: foldedList }]).join(" ") === "a@example.com b@example.com", "a folded list in a draft: " + escaped(foldedList));
        assert(replyRecipients([{ name: "From", value: foldedList }], false).to.join(" ") === "a@example.com b@example.com", "a folded list in a reply: " + escaped(foldedList));
    }

    label("spaces and tabs around an address are removed");
    assert(sendFailure(unsendable([" \tdana@example.com\t "], ["\tlee@example.com"], ["kim@example.com "])).startsWith("invalid_header: "), "entries are accepted");
    assert(draftFailure(unsendable(["dana@example.com\t"], [], [])).startsWith("invalid_header: "), "drafts accept the same entries");
    const stored: Header[] = [
        { name: "From", value: "\t dana@example.com \t" },
        { name: "To", value: "Lee <\tlee@example.com >\t,\r\n\tkim@example.com\t" },
    ];
    assert(draftRecipients(stored).join(" ") === "lee@example.com kim@example.com", "header addresses are read without them");
    assert(replyRecipients(stored, true).to.join(" ") === "dana@example.com lee@example.com kim@example.com", "folded headers still parse");

    label("many recipients are deduplicated in first-seen order");
    const many: string[] = [];
    for (let i = 0; i < 300; i += 1) many.push("user" + (i % 200).toString() + "@example.com");
    const repeated: Header[] = [
        { name: "Bcc", value: "cc@example.com, bcc@example.com" },
        { name: "Cc", value: "user5@example.com, cc@example.com" },
        { name: "To", value: many.join(", ") },
    ];
    const unique = draftRecipients(repeated);
    assert(unique.length === 202, "each address appears once, got " + unique.length.toString());
    assert(unique[0] === "user0@example.com" && unique[199] === "user199@example.com", "To keeps its order");
    assert(unique[200] === "cc@example.com" && unique[201] === "bcc@example.com", "Cc and Bcc follow To");
    const replyAll = replyRecipients([{ name: "From", value: "user7@example.com" }, repeated[1], repeated[2]], true);
    assert(replyAll.to.length === 200 && replyAll.to[0] === "user7@example.com" && replyAll.to[1] === "user0@example.com", "the sender comes first, once");
    assert(replyAll.cc.join(" ") === "cc@example.com", "Cc leaves out every To address");

    label("bare addresses pass, with surrounding spaces removed");
    assert(sendFailure(unsendable([" dana@example.com "], ["lee+notes@mail.example.com"], ["o'neil@example.com"])).startsWith("invalid_header: "), "accepted entries reach message construction");
    assert(draftFailure(unsendable(["dana@example.com"], [], [])).startsWith("invalid_header: "), "optional lists may be empty");
    assert(sendFailure(unsendable([], ["lee@example.com"], [])).startsWith("missing_recipient: "), "Cc alone is not a recipient list");
}

// The subject is one message construction refuses, so an entry accepted by mistake still
// stops before a token is read or a request is sent.
function unsendable(to: string[], cc: string[], bcc: string[]): EmailInput {
    return { to: to, cc: cc, bcc: bcc, subject: "unit test\r\nnever sent", text: "" };
}

// Written as comparisons so it does not repeat the pattern the package matches with. `@` is
// refused here because the entry already holds one.
function isAddressUnit(unit: number): boolean {
    if (unit < 0x21 || unit > 0x7e || unit === 0x40) return false;
    if (unit === 0x22 || unit === 0x28 || unit === 0x29 || unit === 0x2c) return false;
    if (unit >= 0x3a && unit <= 0x3c) return false;
    if (unit === 0x3e) return false;
    return unit < 0x5b || unit > 0x5d;
}

// Assertion messages must show which entry failed, and these entries differ in invisible characters.
function escaped(value: string): string {
    let text = "";
    for (let i = 0; i < value.length; i += 1) {
        const unit = value.charCodeAt(i);
        text += unit >= 0x20 && unit < 0x7f ? value.charAt(i) : "\\u" + unit.toString(16).padStart(4, "0");
    }
    return text;
}

function sendFailure(input: EmailInput): string {
    try {
        sendEmail(input);
    } catch (e) {
        if (e instanceof GmailError) return e.code + ": " + e.message;
        return "unexpected";
    }
    return "sent";
}

function draftFailure(input: EmailInput): string {
    try {
        createDraft(input);
    } catch (e) {
        if (e instanceof GmailError) return e.code + ": " + e.message;
        return "unexpected";
    }
    return "drafted";
}

function replyFailure(sender: string): string {
    try {
        replyRecipients([{ name: "From", value: sender }], false);
    } catch (e) {
        if (e instanceof GmailError) return e.code;
        return "unexpected";
    }
    return "none";
}

function headerFailure(value: string): string {
    try {
        draftRecipients([{ name: "To", value: value }]);
    } catch (e) {
        if (e instanceof GmailError) return e.code;
        return "unexpected";
    }
    return "none";
}
