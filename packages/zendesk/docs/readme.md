# Zendesk package guide

Use `@submilli/zendesk` to work a Zendesk Support help desk: find and read
tickets, reply to customers or leave internal notes, change a ticket's status,
priority, tags and fields, move it between groups and agents, apply macros,
look up customers and organizations, read the Help Center, and read
satisfaction ratings and ticket timings. Credentials and the account's address
are supplied internally; never pass a token or URL. Every call is synchronous
and returns its result.

- Ids are Zendesk's numeric ids as strings, such as `"35436"`, from results
  such as `ticket.id` or `ticket.groupId`. Names are not ids: find a group's id
  with `listGroups`, a field's with `listTicketFields`.
- Zendesk applies its own access: you see and change only what the agent's
  role and groups allow.
- A denial (`PermissionDeniedError`) means the operation is forbidden with
  these arguments, not that it was called wrongly: do not retry it with other
  arguments to get around it. Reading a ticket that does not exist is denied
  the same way as one you may not read.
- A `ZendeskError` with `errorCode` `UpdateConflict` means the ticket changed
  after it was read; read it again and decide again. `retryable` errors (rate
  limits) mean stop and report; a program cannot wait.

<!-- submilli:if any="zendesk.com/comments.reply zendesk.com/comments.note" -->

## Replies and notes

`reply(ticketId, text)` posts a public comment: Zendesk emails it to the
requester and everyone copied. `addNote(ticketId, text)` posts an internal note
that only agents see. When unsure, write a note. `reply` can set the status in
the same write, such as `{ status: "pending" }` to wait for the customer or
`{ status: "solved" }`.

<!-- submilli:endif -->

<!-- submilli:if any="zendesk.com/tickets.find zendesk.com/tickets.count" -->

## Finding tickets

`findTickets(filter, options)` searches tickets; every filter given must match:
`status` (a list, any of them), `groupId`, `assigneeId` (`null` for
unassigned), `requesterId`, `organizationId`, `brandId`, `tag` (one tag),
`createdAfter`, `createdBefore`, `updatedAfter`, `updatedBefore` (`YYYY-MM-DD`
or a date-time with a zone), and `text`, a phrase to find. Results come most
recently changed first; `options.sortBy` and `sortOrder` change that. A page
has `items`, `count` (all matches) and `nextCursor`: pass it back as
`{ cursor }` with the same filter for the next page. Search reads at most 1,000
tickets; `countTickets(filter)` counts without reading them. A ticket just
changed may take a minute to show up in search.

```ts
import { findTickets } from "@submilli/zendesk";

function main(): { id: string; subject: string; waitingSince: string }[] {
    const page = findTickets({ status: ["new", "open"], assigneeId: null }, { sortBy: "created_at", sortOrder: "asc", pageSize: 25 });
    const rows: { id: string; subject: string; waitingSince: string }[] = [];
    for (const ticket of page.items) rows.push({ id: ticket.id, subject: ticket.subject, waitingSince: ticket.createdAt });
    return rows;
}
```

<!-- submilli:endif -->

## Operations

<!-- submilli:if capability="zendesk.com/tickets.get" -->
- `getTicket(id)`: the ticket, or null.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/comments.list" -->
- `listComments(ticketId, { pageSize, cursor })`: the conversation, oldest first; `public` tells replies from notes.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/tickets.create" -->
- `createTicket({ subject, body, public, requesterId, groupId, assigneeId, priority, type, tags, customFields, attachments })`: `public: false` makes the first comment internal.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/tickets.update" -->
- `updateTicket(id, { subject, status, priority, type, tags, addTags, removeTags, customFields, dueAt })`: only what is given changes; `null` clears priority, type or due date. Prefer `addTags`/`removeTags` to `tags`, which replaces them all. Custom fields are `{ id, value }` with ids from `listTicketFields`; a drop-down's value is its option's tag.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/tickets.assign" -->
- `assignTicket(id, { groupId, assigneeId })`: move a ticket; `assigneeId: null` unassigns it. The agent must be in the group: given alone, the agent must be in the ticket's current group.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/tickets.merge" -->
- `mergeTickets(targetId, sourceIds, { targetComment, sourceComment, public })`: up to 5 duplicates into one; the sources are closed. `public: true` emails every requester, so it is also checked as a reply.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/tickets.metrics" -->
- `ticketMetrics(id)`: replies, reopens, and minutes to first reply and to resolution.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/attachments.upload" -->
- `uploadAttachment(path, { fileName, contentType })`: upload a file you wrote; pass its `token` in `attachments` of `reply`, `addNote` or `createTicket`.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/users.get" -->
- `getUser(id)`: a customer (`role: "end-user"`), an agent or an admin.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/users.find" -->
- `findUsers(query)`: by name or email.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/users.create" -->
- `createUser({ name, email, phone, organizationId, tags, notes, details, userFields })`: a customer; `userFields` is a list of `{ key, value }`.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/users.update" -->
- `updateUser(id, { name, phone, tags, notes, details, userFields })`.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/organizations.get" -->
- `getOrganization(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/organizations.find" -->
- `findOrganizations(name)`: organizations whose name starts with it.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/groups.list" -->
- `listGroups()`: the groups of agents and their ids.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/groups.members" -->
- `listAgents(groupId)`: a group's agents.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/views.list" -->
- `listViews()`: saved ticket lists, such as "Unassigned tickets".
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/views.read" -->
- `viewTickets(viewId)` and `countView(viewId)`: a view's tickets in its order, and how many.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/macros.list" -->
- `listMacros()`: the team's saved responses and changes.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/macros.preview" -->
- `previewMacro(ticketId, macroId)`: what a macro would change and the comment it would add, without saving.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/macros.apply" -->
- `applyMacro(ticketId, macroId)`: apply it and save. A macro that adds a public comment replies to the customer. A macro that changes what the package cannot write (CCs, followers, the ticket form) is refused with `unsupported_macro`.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/metadata.read" -->
- `listTicketFields()` and `listBrands()`: field ids and drop-down options, and brands.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/articles.search" -->
- `searchArticles(query, { locale, sectionId })`: Help Center articles, best match first, with a `snippet`.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/articles.get" -->
- `getArticle(id, { locale })`: the article's HTML `body` and its public `htmlUrl`, to quote or link.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/articles.browse" -->
- `listCategories()` and `listSections({ categoryId })`.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/ratings.list" -->
- `listSatisfactionRatings({ score })`: newest first; `score` is `good`, `bad`, `offered`, `unoffered` or `received`.
<!-- submilli:endif -->
<!-- submilli:if capability="zendesk.com/search.raw" -->
- `search(query, { type })`: Zendesk's query language, such as `type:ticket status<solved priority:urgent`; results are untyped. Prefer `findTickets`.
<!-- submilli:endif -->

<!-- submilli:if all="zendesk.com/tickets.get zendesk.com/comments.list zendesk.com/articles.search zendesk.com/comments.note" -->

## Example: draft an answer from the Help Center

```ts
import { addNote, getTicket, listComments, searchArticles } from "@submilli/zendesk";

function main(): string {
    const ticket = getTicket("35436");
    if (ticket === null) return "no such ticket";
    const conversation = listComments(ticket.id);
    const count = conversation.items.length;
    const latest = count > 0 ? conversation.items[count - 1].body : ticket.description;
    const hits = searchArticles(ticket.subject, { pageSize: 3 });
    const links: string[] = [];
    for (const hit of hits.items) links.push("- " + hit.title + ": " + hit.htmlUrl);
    addNote(ticket.id, "For: " + latest.slice(0, 200) + "\nSuggested articles:\n" + links.join("\n"));
    return "noted " + hits.items.length.toString() + " articles";
}
```

<!-- submilli:endif -->
