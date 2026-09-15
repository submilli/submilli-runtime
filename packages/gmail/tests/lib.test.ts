import { label } from "submilli:test";
import {
    GmailError,
    EmailInput,
    ReplyInput,
    LabelChanges,
    SearchOptions,
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
}
