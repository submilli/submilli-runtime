import { get, post, patch, delete, Response } from "submilli:http";
import { encodeComponent, encodeQuery } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const API = "https://www.googleapis.com/calendar/v3";
// Exactly one `@` between two runs of printable ASCII, without the characters RFC 5322 reserves
// for address syntax, `()<>[]:;\,"`. Anything else could be read as a list or a named address
// where policy saw one attendee, or be drawn like an address policy refuses.
const BARE_ADDRESS = /^[!#-'*+\-.\/0-9=?A-Z^_`a-z{|}~]+@[!#-'*+\-.\/0-9=?A-Z^_`a-z{|}~]+$/;
// The dots that end a fully qualified domain. The address is the same mailbox without them.
const TRAILING_DOTS = /\.+$/;

/** A Google Calendar API error with stable machine-readable fields. */
export class CalendarError extends Error {
    code: string;
    status: number;

    constructor(code: string, message: string, status: number) {
        super(message);
        this.name = "CalendarError";
        this.code = code;
        this.status = status;
    }
}

/** Token pagination shared by Calendar list methods. */
export interface PageOptions {
    /** Maximum items per page (clamped to the endpoint's allowed range). */
    limit?: number;
    /** Opaque token from a previous page's `nextPageToken`. */
    pageToken?: string;
}

/** One page of values and the token for the next page. */
export interface Page<T> {
    /** Items on this page. */
    items: T[];
    /** Token for the next page; empty string when this is the last page. */
    nextPageToken: string;
}

/** A calendar visible to the authenticated account. */
export interface Calendar {
    /** Calendar identifier, e.g. "primary" or an email-like ID. */
    id: string;
    /** Calendar title; empty string when unset. */
    summary: string;
    /** Calendar description; empty string when unset. */
    description: string;
    /** IANA time zone of the calendar, e.g. "Europe/London"; empty string when unset. */
    timeZone: string;
    /** The account's access level: "owner", "writer", "reader", or "freeBusyReader"; empty string when unset. */
    accessRole: string;
    /** Whether this is the account's primary calendar. */
    primary: boolean;
    /** Whether the calendar is selected (shown) in the user's UI. */
    selected: boolean;
}

interface ApiCalendar {
    id: string;
    summary?: string | null;
    description?: string | null;
    timeZone?: string | null;
    accessRole?: string | null;
    primary?: boolean | null;
    selected?: boolean | null;
}

interface CalendarListResponse {
    items?: ApiCalendar[];
    nextPageToken?: string;
}

/** A date-only or date-time Calendar endpoint. */
export interface EventTime {
    /** All-day date, "YYYY-MM-DD". Set for all-day events; mutually exclusive with `dateTime`. */
    date?: string;
    /** RFC 3339 timestamp with offset, e.g. "2026-08-10T09:00:00+02:00"; Temporal strings with a bracketed zone are accepted and normalized. Mutually exclusive with `date`. */
    dateTime?: string;
    /** IANA time zone the time is interpreted in, e.g. "America/New_York". Required for recurring events. */
    timeZone?: string;
}

/** An event attendee. */
export interface Attendee {
    /**
     * Attendee email address, one bare address such as "dana@example.com". It is checked and sent in
     * lowercase. Empty in a returned event when Calendar lists the attendee without an address.
     */
    email: string;
    /** Attendee display name. */
    displayName?: string;
    /** Whether attendance is optional. */
    optional?: boolean;
    /** RSVP: "needsAction", "accepted", "declined", or "tentative". */
    responseStatus?: string;
    /** Attendee's RSVP note. */
    comment?: string;
    /** Whether this attendee is the authenticated account. */
    self?: boolean;
}

/** A Calendar reminder override. */
export interface Reminder {
    /** Delivery method: "email" or "popup". */
    method: string;
    /** Minutes before the event start to fire the reminder. */
    minutes: number;
}

/** Reminder settings for an event. */
export interface ReminderSettings {
    /** Use the calendar's default reminders instead of `overrides`. */
    useDefault: boolean;
    /** Explicit reminders; only honored when `useDefault` is false. */
    overrides?: Reminder[];
}

/** Curated Google Calendar event data. */
export interface Event {
    /** Event identifier, unique within its calendar. */
    id: string;
    /** "confirmed", "tentative", or "cancelled"; empty string when unset. */
    status: string;
    /** Link to the event in the Google Calendar web UI; empty string when unset. */
    htmlLink: string;
    /** Event title; empty string when unset. */
    summary: string;
    /** Event description; empty string when unset. */
    description: string;
    /** Free-form location; empty string when unset. */
    location: string;
    /** Event start (all-day date or timed dateTime); empty for a cancelled instance listed with `showDeleted`. */
    start: EventTime;
    /** Event end, exclusive (all-day date or timed dateTime); empty for a cancelled instance listed with `showDeleted`. */
    end: EventTime;
    /** Attendees; empty array when none. */
    attendees: Attendee[];
    /** RRULE/EXRULE/RDATE/EXDATE lines (RFC 5545); empty array for non-recurring events. */
    recurrence: string[];
    /** For an instance of a recurring event, the ID of the recurring parent; empty string otherwise. */
    recurringEventId: string;
    /** "default", "public", "private", or "confidential"; empty string when unset. */
    visibility: string;
    /** Google Meet link; empty string when the event has none. */
    hangoutLink: string;
    /** Creation time, RFC 3339; empty string when unset. */
    created: string;
    /** Last modification time, RFC 3339; empty string when unset. */
    updated: string;
}

interface ApiEvent {
    id: string;
    status?: string | null;
    htmlLink?: string | null;
    summary?: string | null;
    description?: string | null;
    location?: string | null;
    // A cancelled instance of a recurring event, listed with `showDeleted`, carries neither.
    start?: EventTime | null;
    end?: EventTime | null;
    attendees?: ApiAttendee[] | null;
    recurrence?: string[] | null;
    recurringEventId?: string | null;
    visibility?: string | null;
    hangoutLink?: string | null;
    created?: string | null;
    updated?: string | null;
    attendeesOmitted?: boolean | null;
}

// Calendar can list an attendee without an address, such as a removed account.
interface ApiAttendee {
    email?: string | null;
    displayName?: string | null;
    optional?: boolean | null;
    responseStatus?: string | null;
    comment?: string | null;
    self?: boolean | null;
}

interface EventListResponse {
    items?: ApiEvent[];
    nextPageToken?: string;
}

interface EventAttendeeMetadata {
    attendees?: ApiAttendee[] | null;
    attendeesOmitted?: boolean | null;
}

/** Filters and pagination for listing events. */
export interface EventListOptions {
    /** Calendar to list from; defaults to "primary". */
    calendarId?: string;
    /** Maximum events per page, 1-2500; defaults to 20. */
    limit?: number;
    /** Opaque token from a previous page's `nextPageToken`. */
    pageToken?: string;
    /** Lower bound (exclusive) on event end time, RFC 3339; bracketed Temporal strings are normalized to UTC. */
    timeMin?: string;
    /** Upper bound (exclusive) on event start time, RFC 3339; bracketed Temporal strings are normalized to UTC. */
    timeMax?: string;
    /** Free-text search over event fields. */
    query?: string;
    /** Expand recurring events into individual instances. */
    singleEvents?: boolean;
    /** Sort order: "startTime" (requires `singleEvents`) or "updated". */
    orderBy?: string;
    /** Include cancelled events. */
    showDeleted?: boolean;
    /** IANA time zone used for times in the response; defaults to the calendar's zone. */
    timeZone?: string;
}

/** Fields accepted when creating an event. */
export interface EventCreateInput {
    /** Event title. */
    summary: string;
    /** Event start (all-day `date` or timed `dateTime`). */
    start: EventTime;
    /** Event end, exclusive (all-day `date` or timed `dateTime`). */
    end: EventTime;
    /** Event description. */
    description?: string;
    /** Free-form location. */
    location?: string;
    /** Attendees to invite. */
    attendees?: Attendee[];
    /** RRULE/EXRULE/RDATE/EXDATE lines (RFC 5545) for a recurring event. */
    recurrence?: string[];
    /** Reminder settings; omit for the calendar's defaults. */
    reminders?: ReminderSettings;
    /** "default", "public", "private", or "confidential". */
    visibility?: string;
    /** Attach a Google Meet conference to the event. */
    createGoogleMeet?: boolean;
    /** Who receives invitation emails: "all", "externalOnly", or "none". */
    sendUpdates?: string;
}

interface EventCreateBody {
    summary: string;
    start: EventTime;
    end: EventTime;
    description?: string;
    location?: string;
    attendees?: Attendee[];
    recurrence?: string[];
    reminders?: ReminderSettings;
    visibility?: string;
    conferenceData?: ConferenceData;
}

interface ConferenceData {
    createRequest: ConferenceRequest;
}

interface ConferenceRequest {
    requestId: string;
    conferenceSolutionKey: ConferenceSolutionKey;
}

interface ConferenceSolutionKey {
    type: string;
}

/** Patch fields for an existing event. Explicit clear flags remove description/location. */
export interface EventUpdateInput {
    /** New event title. */
    summary?: string;
    /** New start (all-day `date` or timed `dateTime`). */
    start?: EventTime;
    /** New end, exclusive (all-day `date` or timed `dateTime`). */
    end?: EventTime;
    /** New description; ignored when `clearDescription` is true. */
    description?: string;
    /** New free-form location; ignored when `clearLocation` is true. */
    location?: string;
    /** Remove the description from the event. */
    clearDescription?: boolean;
    /** Remove the location from the event. */
    clearLocation?: boolean;
    /** Replacement attendee list; an empty array clears attendees. */
    attendees?: Attendee[];
    /** Replacement RRULE/EXRULE/RDATE/EXDATE lines (RFC 5545). */
    recurrence?: string[];
    /** Replacement reminder settings. */
    reminders?: ReminderSettings;
    /** "default", "public", "private", or "confidential". */
    visibility?: string;
    /** Who receives change emails: "all", "externalOnly", or "none". */
    sendUpdates?: string;
}

interface EventUpdateBody {
    summary?: string;
    start?: EventTime;
    end?: EventTime;
    description?: string | null;
    location?: string | null;
    attendees?: Attendee[];
    recurrence?: string[];
    reminders?: ReminderSettings;
    visibility?: string;
}

/** Options for event deletion. */
export interface EventDeleteOptions {
    /** Calendar holding the event; defaults to "primary". */
    calendarId?: string;
    /** Who receives cancellation emails: "all", "externalOnly", or "none". */
    sendUpdates?: string;
}

/** One busy interval. */
export interface BusyPeriod {
    /** Interval start, RFC 3339. */
    start: string;
    /** Interval end (exclusive), RFC 3339. */
    end: string;
}

/** Free/busy query input. */
export interface FreeBusyInput {
    /** Calendars to query, 1-50 IDs. */
    calendarIds: string[];
    /** Range start, RFC 3339; bracketed Temporal strings are normalized to UTC. */
    timeMin: string;
    /** Range end (exclusive), RFC 3339; bracketed Temporal strings are normalized to UTC. */
    timeMax: string;
    /** IANA time zone for the response; defaults to UTC. */
    timeZone?: string;
}

interface FreeBusyRequest {
    timeMin: string;
    timeMax: string;
    items: FreeBusyItem[];
    timeZone?: string;
}

interface FreeBusyItem {
    id: string;
}

interface ApiFreeBusyCalendar {
    busy?: BusyPeriod[] | null;
}

interface FreeBusyResponse {
    calendars: {};
}

/** Busy intervals for a single calendar. */
export interface CalendarBusy {
    /** The queried calendar's ID. */
    calendarId: string;
    /** Busy intervals within the queried range; empty array when fully free. */
    busy: BusyPeriod[];
}

/** Normalized free/busy query result. */
export interface FreeBusyResult {
    /** Queried range start, RFC 3339 UTC (input normalized). */
    timeMin: string;
    /** Queried range end (exclusive), RFC 3339 UTC (input normalized). */
    timeMax: string;
    /** Per-calendar busy intervals, in the order the calendars were requested. */
    calendars: CalendarBusy[];
}

/** Input for the bounded free-slot helper. */
export interface FindFreeTimeInput {
    /** Calendars whose busy intervals must all be avoided, 1-50 IDs. */
    calendarIds: string[];
    /** Search window start, RFC 3339; bracketed Temporal strings are normalized to UTC. */
    timeMin: string;
    /** Search window end (exclusive), RFC 3339; bracketed Temporal strings are normalized to UTC. */
    timeMax: string;
    /** Required slot length in minutes; must be positive. */
    durationMinutes: number;
    /** IANA time zone for the underlying free/busy query. */
    timeZone?: string;
    /** Maximum slots to return, 1-100; defaults to 10. */
    maxSlots?: number;
}

/** A candidate free interval. */
export interface TimeSlot {
    /** Slot start, RFC 3339. */
    start: string;
    /** Slot end (start + requested duration), RFC 3339. */
    end: string;
}

/** Input for a bounded multi-calendar agenda. */
export interface AgendaOptions {
    /** Agenda window start, RFC 3339; bracketed Temporal strings are normalized to UTC. */
    timeMin: string;
    /** Agenda window end (exclusive), RFC 3339; bracketed Temporal strings are normalized to UTC. */
    timeMax: string;
    /** Explicit calendars to include; omit to use the account's visible calendars. */
    calendarIds?: string[];
    /** Maximum calendars to include, 1-10; defaults to 10. */
    maxCalendars?: number;
    /** Maximum events fetched per calendar, 1-100; defaults to 20. */
    maxEventsPerCalendar?: number;
    /** IANA time zone used for times in the returned events. */
    timeZone?: string;
}

/** An agenda event annotated with its source calendar. */
export interface AgendaEvent {
    /** Calendar the event came from. */
    calendarId: string;
    /** The event itself. */
    event: Event;
}

/** Bounded, sorted agenda output. */
export interface AgendaResult {
    /** Events across all included calendars, sorted by start time. */
    events: AgendaEvent[];
    /** True when calendars or events were dropped by the bounds (more data exists). */
    truncated: boolean;
}

interface GoogleErrorEnvelope {
    error?: GoogleErrorBody;
}

interface GoogleErrorBody {
    message?: string;
    status?: string;
    errors?: GoogleErrorDetail[];
}

interface GoogleErrorDetail {
    reason?: string;
}

/**
 * List calendars visible to the authenticated account.
  * @param page Optional page size (1-250, default 20) and continuation token; omit it to use the defaults.
  * @returns One page of calendars; `nextPageToken` is empty on the last page.
 * @capability submilli/google-calendar.listCalendars {}
 */
export function listCalendars(page: PageOptions = {}): Page<Calendar> {
    const { limit, pageToken } = page;
    check("submilli/google-calendar.listCalendars", {});
    const query = new Map<string, string>();
    applyPage(query, limit, pageToken, 20, 250);
    const data = calendarGet("/users/me/calendarList", query).json() as CalendarListResponse;
    const items: Calendar[] = [];
    const calendars = data.items;
    if (calendars !== undefined) for (const item of calendars) items.push(calendarFrom(item));
    return { items: items, nextPageToken: data.nextPageToken ?? "" };
}

/**
 * List one page of events. The default calendar is `primary`.
  * @param options Optional calendar ID (default `primary`), page size, time bounds, text query, ordering, and other filters; omit it to list the primary calendar with the API defaults.
  * @returns One page of events; `items` is empty when nothing matches and `nextPageToken` is empty on the last page.
 * @capability submilli/google-calendar.listEvents { calendarId: string }
 */
export function listEvents(options: EventListOptions = {}): Page<Event> {
    const {
        calendarId = "primary",
        limit,
        pageToken,
        timeMin,
        timeMax,
        query: search,
        singleEvents,
        orderBy,
        showDeleted,
        timeZone,
    } = options;
    check("submilli/google-calendar.listEvents", { calendarId: calendarId });
    const query = new Map<string, string>();
    query.set("maxResults", bounded(limit, 20, 1, 2500).toString());
    putQuery(query, "pageToken", pageToken);
    putTimeQuery(query, "timeMin", timeMin);
    putTimeQuery(query, "timeMax", timeMax);
    putQuery(query, "q", search);
    putBool(query, "singleEvents", singleEvents);
    putQuery(query, "orderBy", orderBy);
    putBool(query, "showDeleted", showDeleted);
    putQuery(query, "timeZone", timeZone);
    const data = calendarGet("/calendars/" + encodeComponent(calendarId) + "/events", query).json() as EventListResponse;
    return eventPage(data);
}

/**
 * Fetch one event, returning null when it does not exist.
  * @param eventId Event ID within the calendar.
  * @param calendarId Calendar holding the event, such as `primary` or a calendar's email-like ID.
  * @returns The event, or `null` when it does not exist.
 * @capability submilli/google-calendar.getEvent { calendarId: string }
 */
export function getEvent(eventId: string, calendarId: string = "primary"): Event | null {
    check("submilli/google-calendar.getEvent", { calendarId: calendarId });
    return fetchEvent(eventId, calendarId);
}

function fetchEvent(eventId: string, calendarId: string): Event | null {
    const item = fetchApiEvent(eventId, calendarId);
    return item === null ? null : eventFrom(item);
}

function fetchApiEvent(eventId: string, calendarId: string): ApiEvent | null {
    const response = calendarRawGet("/calendars/" + encodeComponent(calendarId) + "/events/" + encodeComponent(eventId), new Map<string, string>());
    if (response.status === 404) return null;
    requireOk(response);
    return response.json() as ApiEvent;
}

/**
 * Create a Calendar event and optionally request a Google Meet conference. `attendees` in the
 * check holds each attendee's address once, in lowercase. `sendUpdates` is "none" when unset.
  * @param input Title, start and end, and optional description, location, attendees, recurrence, reminders, visibility, Meet request, and notification mode.
  * @param calendarId Calendar to create the event in; defaults to `primary`.
  * @returns The created event, including its `id` and, when requested, its Google Meet link.
 * @capability submilli/google-calendar.createEvent { calendarId: string, attendees: string[], sendUpdates: string }
 */
export function createEvent(input: EventCreateInput, calendarId: string = "primary"): Event {
    const { summary, description, location, visibility, createGoogleMeet, sendUpdates: requestedSendUpdates } = input;
    const sendUpdates = sendUpdatesMode(requestedSendUpdates);
    const start = copyEventTime(input.start);
    const end = copyEventTime(input.end);
    const requestedAttendees = input.attendees;
    const attendees: Attendee[] = [];
    if (requestedAttendees !== undefined) {
        // Each attendee is copied with its address in the one spelling policy reads, so the
        // list the check approves is the list the request sends.
        for (const item of requestedAttendees) {
            const { email, displayName, optional, responseStatus, comment } = item;
            attendees.push({
                email: validAttendeeAddress(email),
                displayName: displayName,
                optional: optional,
                responseStatus: responseStatus,
                comment: comment,
            });
        }
    }
    const recurrence = copyStrings(input.recurrence);
    const reminders = input.reminders;
    check("submilli/google-calendar.createEvent", { calendarId: calendarId, attendees: distinctAddresses(attendees), sendUpdates: sendUpdates });
    const body: EventCreateBody = {
        summary: summary,
        start: normalizeEventTime(start),
        end: normalizeEventTime(end),
        description: description,
        location: location,
        recurrence: recurrence,
        reminders: reminders,
        visibility: visibility,
    };
    if (attendees.length > 0) body.attendees = attendees;
    const query = new Map<string, string>();
    query.set("sendUpdates", sendUpdates);
    if (createGoogleMeet === true) {
        query.set("conferenceDataVersion", "1");
        body.conferenceData = {
            createRequest: {
                requestId: meetRequestId(),
                conferenceSolutionKey: { type: "hangoutsMeet" },
            },
        };
    }
    const response = post(API + calendarPath("/calendars/" + encodeComponent(calendarId) + "/events", query), body, authHeaders());
    requireOk(response);
    return eventFrom(response.json() as ApiEvent);
}

/**
 * Patch an event. Omitted fields remain unchanged; empty attendees clears attendees. `attendees`
 * in the check holds the address of each attendee the event has after the update, once, in
 * lowercase: the replacement list, or the event's current attendees when the patch has none.
 * The event is read for that before the check. `sendUpdates` is "none" when unset.
  * @param eventId ID of the event to patch.
  * @param input Fields to change; omitted fields keep their current values.
  * @param calendarId Calendar holding the event; defaults to `primary`.
  * @returns The event after the update.
 * @capability submilli/google-calendar.updateEvent { calendarId: string, attendees: string[], removedAttendees: string[], notificationRecipients: string[], sendUpdates: string }
 */
export function updateEvent(eventId: string, input: EventUpdateInput, calendarId: string = "primary"): Event {
    const { summary, description, location, clearDescription, clearLocation, visibility, sendUpdates: requestedSendUpdates } = input;
    const sendUpdates = sendUpdatesMode(requestedSendUpdates);
    const requestedStart = input.start;
    const start = requestedStart === undefined ? undefined : copyEventTime(requestedStart);
    const requestedEnd = input.end;
    const end = requestedEnd === undefined ? undefined : copyEventTime(requestedEnd);
    const requestedAttendees = input.attendees;
    let attendees: Attendee[] | undefined;
    if (requestedAttendees !== undefined) {
        attendees = [];
        // Each attendee is copied with its address in the one spelling policy reads, so the
        // list the check approves is the list the request sends.
        for (const item of requestedAttendees) {
            const { email, displayName, optional, responseStatus, comment } = item;
            attendees.push({
                email: validAttendeeAddress(email),
                displayName: displayName,
                optional: optional,
                responseStatus: responseStatus,
                comment: comment,
            });
        }
    }
    const recurrence = copyStrings(input.recurrence);
    const reminders = input.reminders;
    const attendeesAfterUpdate = attendees !== undefined ? distinctAddresses(attendees) : currentAttendeeAddresses(eventId, calendarId);
    const removedAttendees: string[] = [];
    let notificationRecipients: string[] = [];
    if (sendUpdates !== "none") {
        const previous = attendees === undefined ? attendeesAfterUpdate : currentAttendeeAddresses(eventId, calendarId);
        for (const address of previous) {
            if (!attendeesAfterUpdate.includes(address)) removedAttendees.push(address);
        }
        notificationRecipients = distinct(previous.concat(attendeesAfterUpdate));
    }
    check("submilli/google-calendar.updateEvent", {
        calendarId: calendarId, attendees: attendeesAfterUpdate,
        removedAttendees: removedAttendees, notificationRecipients: notificationRecipients, sendUpdates: sendUpdates,
    });
    const body: EventUpdateBody = {
        summary: summary,
        start: start === undefined ? undefined : normalizeEventTime(start),
        end: end === undefined ? undefined : normalizeEventTime(end),
        attendees: attendees,
        recurrence: recurrence,
        reminders: reminders,
        visibility: visibility,
    };
    // Calendar removes a field the patch sets to null.
    if (clearDescription === true) {
        body.description = null;
    } else if (description !== undefined) {
        body.description = description;
    }
    if (clearLocation === true) {
        body.location = null;
    } else if (location !== undefined) {
        body.location = location;
    }
    const query = new Map<string, string>();
    query.set("sendUpdates", sendUpdates);
    const path = "/calendars/" + encodeComponent(calendarId) + "/events/" + encodeComponent(eventId);
    const response = patch(API + calendarPath(path, query), body, authHeaders());
    requireOk(response);
    return eventFrom(response.json() as ApiEvent);
}

/**
 * Set the authenticated attendee's response status on an event.
  * @param eventId ID of the event to respond to.
  * @param response RSVP status: `accepted`, `declined`, `tentative`, or `needsAction`.
  * @param calendarId Calendar holding the event; defaults to `primary`.
  * @returns The event with the authenticated account's response status updated.
 * @capability submilli/google-calendar.respondToEvent { calendarId: string, response: string }
 */
export function respondToEvent(eventId: string, response: string, calendarId: string = "primary"): Event {
    check("submilli/google-calendar.respondToEvent", { calendarId: calendarId, response: response });
    if (response !== "accepted" && response !== "declined" && response !== "tentative" && response !== "needsAction") {
        throw new CalendarError("invalid_response", "response must be accepted, declined, tentative, or needsAction", 0);
    }
    const current = fetchApiEvent(eventId, calendarId);
    if (current === null) throw new CalendarError("not_found", "Calendar event was not found", 404);
    // The patch replaces the attendee list, so it resends every attendee as Calendar listed it,
    // including any without an address.
    let attendees: ApiAttendee[] = [];
    if (current.attendees) attendees = current.attendees;
    let foundSelf = false;
    for (const attendee of attendees) {
        if (attendee.self === true) {
            attendee.responseStatus = response;
            foundSelf = true;
        }
    }
    if (!foundSelf) throw new CalendarError("self_attendee_not_found", "the authenticated account is not an attendee on this event", 0);
    const query = new Map<string, string>();
    query.set("sendUpdates", "all");
    const path = "/calendars/" + encodeComponent(calendarId) + "/events/" + encodeComponent(eventId);
    const result = patch(API + calendarPath(path, query), { attendees: attendees }, authHeaders());
    requireOk(result);
    return eventFrom(result.json() as ApiEvent);
}

/**
 * Delete an event. This is idempotent when the event is already absent. `sendUpdates` in the
 * check is "none" when unset.
  * @param eventId ID of the event to delete.
  * @param options Optional calendar ID (default `primary`) and notification mode (default `none`); omit it to use the defaults.
 * @capability submilli/google-calendar.deleteEvent { calendarId: string, sendUpdates: string }
 */
export function deleteEvent(eventId: string, options: EventDeleteOptions = {}): void {
    const { calendarId = "primary", sendUpdates: requestedSendUpdates } = options;
    const sendUpdates = sendUpdatesMode(requestedSendUpdates);
    check("submilli/google-calendar.deleteEvent", { calendarId: calendarId, sendUpdates: sendUpdates });
    const query = new Map<string, string>();
    query.set("sendUpdates", sendUpdates);
    const path = "/calendars/" + encodeComponent(calendarId) + "/events/" + encodeComponent(eventId);
    const response = delete(API + calendarPath(path, query), authHeaders());
    if (response.status !== 404) requireOk(response);
}

// A patch without `attendees` leaves the event's attendees in place, and they are the ones
// `sendUpdates` notifies, so they are read from the event for the check.
function currentAttendeeAddresses(eventId: string, calendarId: string): string[] {
    const query = new Map<string, string>();
    query.set("fields", "attendees(email),attendeesOmitted");
    const response = calendarRawGet("/calendars/" + encodeComponent(calendarId) + "/events/" + encodeComponent(eventId), query);
    if (response.status === 404) throw new CalendarError("not_found", "Calendar event was not found", 404);
    requireOk(response);
    const current = response.json() as EventAttendeeMetadata;
    if (current.attendeesOmitted === true) {
        throw new CalendarError("incomplete_attendees", "Calendar omitted attendees; notification recipients cannot be authorized", 0);
    }
    const addresses: string[] = [];
    const attendees = current.attendees;
    if (attendees) {
        for (const attendee of attendees) {
            const email = attendee.email;
            // Calendar can list an attendee without an address; there is nobody to notify.
            if (email) addresses.push(oneSpelling(email));
        }
    }
    return distinct(addresses);
}

// The package's own copy of a caller's `EventTime`, each property read once.
function copyEventTime(time: EventTime): EventTime {
    const { date, dateTime, timeZone } = time;
    return { date: date, dateTime: dateTime, timeZone: timeZone };
}

function copyStrings(values: string[] | undefined): string[] | undefined {
    if (values === undefined) return undefined;
    const copied: string[] = [];
    for (const value of values) copied.push(value);
    return copied;
}

function distinctAddresses(attendees: Attendee[]): string[] {
    const addresses: string[] = [];
    for (const attendee of attendees) addresses.push(attendee.email);
    return distinct(addresses);
}

function validAttendeeAddress(email: string): string {
    if (!BARE_ADDRESS.test(email)) throw invalidAttendee();
    const address = oneSpelling(email);
    if (address.endsWith("@")) throw invalidAttendee();
    return address;
}

// Mail systems deliver to a mailbox whatever the case of its address and whether or not a dot
// ends its domain, so a rule naming one spelling would let another past.
function oneSpelling(address: string): string {
    return address.toLowerCase().replace(TRAILING_DOTS, "");
}

function invalidAttendee(): CalendarError {
    return new CalendarError("invalid_attendee", "an attendee email must be one bare address, such as dana@example.com", 0);
}

function distinct(values: string[]): string[] {
    const seen = new Set<string>();
    const result: string[] = [];
    for (const value of values) {
        if (seen.has(value)) continue;
        seen.add(value);
        result.push(value);
    }
    return result;
}

// Calendar sends no email when `sendUpdates` is absent, which is "none".
function sendUpdatesMode(requested: string | undefined): string {
    if (requested === undefined) return "none";
    if (requested !== "all" && requested !== "externalOnly" && requested !== "none") {
        throw new CalendarError("invalid_send_updates", "sendUpdates must be all, externalOnly, or none", 0);
    }
    return requested;
}

/**
 * Query busy intervals for explicit calendars and a bounded time range.
  * @param input Calendar IDs (1-50), the time range, and an optional response time zone.
  * @returns The normalized range and, for each requested calendar in order, its busy intervals; an empty `busy` array means the calendar is free throughout.
 * @capability submilli/google-calendar.queryFreeBusy { calendarIds: string[] }
 */
export function queryFreeBusy(input: FreeBusyInput): FreeBusyResult {
    const { timeMin: requestedMin, timeMax: requestedMax, timeZone } = input;
    const calendarIds: string[] = [];
    for (const id of input.calendarIds) calendarIds.push(id);
    check("submilli/google-calendar.queryFreeBusy", { calendarIds: calendarIds });
    if (calendarIds.length === 0) throw new CalendarError("invalid_input", "calendarIds must not be empty", 0);
    if (calendarIds.length > 50) throw new CalendarError("too_many_calendars", "free/busy accepts at most 50 calendars", 0);
    const timeMin = toRfc3339(requestedMin, "timeMin");
    const timeMax = toRfc3339(requestedMax, "timeMax");
    const calendars: CalendarBusy[] = [];
    for (const id of calendarIds) {
        const body: FreeBusyRequest = {
            timeMin: timeMin,
            timeMax: timeMax,
            items: [{ id: id }],
        };
        if (timeZone !== undefined) body.timeZone = timeZone;
        const response = post(API + "/freeBusy", body, authHeaders());
        requireOk(response);
        const data = response.json() as FreeBusyResponse;
        const values = Object.values(data.calendars);
        let busy: BusyPeriod[] = [];
        if (values.length > 0) {
            const value = values[0] as ApiFreeBusyCalendar;
            const periods = value.busy;
            if (periods) busy = periods;
        }
        calendars.push({ calendarId: id, busy: busy });
    }
    return { timeMin: timeMin, timeMax: timeMax, calendars: calendars };
}

/**
 * Find bounded candidate slots by merging busy intervals from multiple calendars.
  * @param input Calendar IDs, the search window, the required slot length in minutes, and optional time zone and slot cap.
  * @returns Chronological candidate slots of the requested length that avoid every busy interval; empty when no gap is long enough.
 * @capability submilli/google-calendar.findFreeTime { calendarIds: string[] }
 */
export function findFreeTime(input: FindFreeTimeInput): TimeSlot[] {
    const { timeMin: requestedMin, timeMax: requestedMax, durationMinutes, timeZone, maxSlots: requestedSlots } = input;
    const calendarIds: string[] = [];
    for (const id of input.calendarIds) calendarIds.push(id);
    check("submilli/google-calendar.findFreeTime", { calendarIds: calendarIds });
    if (durationMinutes <= 0) throw new CalendarError("invalid_duration", "durationMinutes must be positive", 0);
    const timeMin = toRfc3339(requestedMin, "timeMin");
    const timeMax = toRfc3339(requestedMax, "timeMax");
    const query: FreeBusyInput = {
        calendarIds: calendarIds,
        timeMin: timeMin,
        timeMax: timeMax,
    };
    if (timeZone !== undefined) query.timeZone = timeZone;
    const result = queryFreeBusy(query);
    const busy: BusyPeriod[] = [];
    for (const calendar of result.calendars) for (const period of calendar.busy) busy.push(period);
    busy.sort((a: BusyPeriod, b: BusyPeriod): number => a.start < b.start ? -1 : a.start > b.start ? 1 : 0);
    const merged: BusyPeriod[] = [];
    for (const period of busy) {
        if (merged.length === 0) {
            merged.push(period);
        } else {
            const last = merged[merged.length - 1];
            if (period.start <= last.end) {
                if (period.end > last.end) last.end = period.end;
            } else {
                merged.push(period);
            }
        }
    }
    const maxSlots = bounded(requestedSlots, 10, 1, 100);
    const slots: TimeSlot[] = [];
    let cursor = timeMin;
    for (const period of merged) {
        if (slots.length >= maxSlots) break;
        if (period.start > cursor && minutesBetween(cursor, period.start) >= durationMinutes) {
            slots.push({ start: cursor, end: addMinutes(cursor, durationMinutes) });
        }
        if (period.end > cursor) cursor = period.end;
    }
    if (slots.length < maxSlots && cursor < timeMax && minutesBetween(cursor, timeMax) >= durationMinutes) {
        slots.push({ start: cursor, end: addMinutes(cursor, durationMinutes) });
    }
    return slots;
}

/**
 * Build a bounded chronological agenda across explicit calendars or up to ten visible calendars.
  * @param options Time window and optional calendar IDs, per-calendar bounds, and time zone; omitted calendar IDs use the account's visible calendars.
  * @returns Events from all included calendars sorted by start time; `truncated` is true when calendars or events were dropped by the bounds.
 * @capability submilli/google-calendar.agenda {}
 */
export function agenda(options: AgendaOptions): AgendaResult {
    const {
        timeMin: requestedMin,
        timeMax: requestedMax,
        calendarIds: requestedCalendarIds,
        maxCalendars,
        maxEventsPerCalendar,
        timeZone,
    } = options;
    const calendarIds = copyStrings(requestedCalendarIds);
    check("submilli/google-calendar.agenda", {});
    const calendarLimit = bounded(maxCalendars, 10, 1, 10);
    const eventLimit = bounded(maxEventsPerCalendar, 20, 1, 100);
    const timeMin = toRfc3339(requestedMin, "timeMin");
    const timeMax = toRfc3339(requestedMax, "timeMax");
    const ids: string[] = [];
    if (calendarIds !== undefined) {
        for (const id of calendarIds) if (ids.length < calendarLimit) ids.push(id);
    } else {
        const page = listCalendars({ limit: calendarLimit });
        for (const calendar of page.items) ids.push(calendar.id);
    }
    const events: AgendaEvent[] = [];
    let truncated = false;
    if (calendarIds !== undefined) truncated = calendarIds.length > ids.length;
    for (const id of ids) {
        const page = listEvents({
            calendarId: id,
            limit: eventLimit,
            timeMin: timeMin,
            timeMax: timeMax,
            singleEvents: true,
            orderBy: "startTime",
            timeZone: timeZone,
        });
        for (const event of page.items) events.push({ calendarId: id, event: event });
        if (page.nextPageToken.length > 0) truncated = true;
    }
    events.sort((a: AgendaEvent, b: AgendaEvent): number => eventStart(a.event) < eventStart(b.event) ? -1 : eventStart(a.event) > eventStart(b.event) ? 1 : 0);
    return { events: events, truncated: truncated };
}

function eventPage(data: EventListResponse): Page<Event> {
    const items: Event[] = [];
    const events = data.items;
    if (events !== undefined) for (const item of events) items.push(eventFrom(item));
    return { items: items, nextPageToken: data.nextPageToken ?? "" };
}

function eventFrom(item: ApiEvent): Event {
    const attendees: Attendee[] = [];
    const listed = item.attendees;
    if (listed) for (const attendee of listed) attendees.push(attendeeFrom(attendee));
    let recurrence: string[] = [];
    if (item.recurrence) recurrence = item.recurrence;
    return {
        id: item.id,
        status: item.status ?? "",
        htmlLink: item.htmlLink ?? "",
        summary: item.summary ?? "",
        description: item.description ?? "",
        location: item.location ?? "",
        start: item.start ?? {},
        end: item.end ?? {},
        attendees: attendees,
        recurrence: recurrence,
        recurringEventId: item.recurringEventId ?? "",
        visibility: item.visibility ?? "",
        hangoutLink: item.hangoutLink ?? "",
        created: item.created ?? "",
        updated: item.updated ?? "",
    };
}

function attendeeFrom(item: ApiAttendee): Attendee {
    return {
        email: item.email ?? "",
        displayName: item.displayName ?? undefined,
        optional: item.optional ?? undefined,
        responseStatus: item.responseStatus ?? undefined,
        comment: item.comment ?? undefined,
        self: item.self ?? undefined,
    };
}

function calendarFrom(item: ApiCalendar): Calendar {
    return {
        id: item.id,
        summary: item.summary ?? "",
        description: item.description ?? "",
        timeZone: item.timeZone ?? "",
        accessRole: item.accessRole ?? "",
        primary: item.primary === true,
        selected: item.selected === true,
    };
}

function calendarGet(path: string, query: Map<string, string>): Response {
    return requireOk(calendarRawGet(path, query));
}

function calendarRawGet(path: string, query: Map<string, string>): Response {
    return get(API + calendarPath(path, query), authHeaders());
}

function calendarPath(path: string, query: Map<string, string>): string {
    const encoded = encodeQuery(query);
    return path + (encoded.length > 0 ? "?" + encoded : "");
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("GOOGLE_ACCESS_TOKEN");
    if (token === undefined) throw new CalendarError("missing_token", "GOOGLE_ACCESS_TOKEN is not bound", 0);
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    return headers;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    let code = response.status === 401 ? "unauthorized" : "http_error";
    let message = "Google Calendar request failed: HTTP " + response.status.toString() + " " + response.statusText;
    if (response.body.startsWith("{")) {
        const envelope = response.json() as GoogleErrorEnvelope;
        const body = envelope.error;
        if (body !== undefined) {
            const errors = body.errors;
            const bodyStatus = body.status;
            const bodyMessage = body.message;
            if (errors !== undefined && errors.length > 0) {
                const reason = errors[0].reason;
                if (reason !== undefined) code = reason;
            } else if (bodyStatus !== undefined) {
                code = bodyStatus;
            }
            if (bodyMessage !== undefined) message = bodyMessage;
        }
    }
    throw new CalendarError(code, message, response.status);
}

function applyPage(query: Map<string, string>, limit: number | undefined, pageToken: string | undefined, defaultLimit: number, maxLimit: number): void {
    query.set("maxResults", bounded(limit, defaultLimit, 1, maxLimit).toString());
    putQuery(query, "pageToken", pageToken);
}

function putQuery(query: Map<string, string>, name: string, value: string | undefined): void {
    if (value !== undefined) query.set(name, value);
}

function putTimeQuery(query: Map<string, string>, name: string, value: string | undefined): void {
    if (value !== undefined) query.set(name, toRfc3339(value, name));
}

function toRfc3339(value: string, param: string): string {
    try {
        return Temporal.Instant.from(value).toString();
    } catch (e) {
        throw new CalendarError("invalid_timestamp", param + " is not an RFC 3339 timestamp: " + (e as Error).message, 0);
    }
}

function normalizeEventTime(time: EventTime): EventTime {
    const { date, dateTime, timeZone } = time;
    if (dateTime === undefined) return { date: date, timeZone: timeZone };
    let zone = timeZone;
    if (zone === undefined && dateTime.indexOf("[") >= 0) {
        zone = Temporal.ZonedDateTime.from(dateTime).timeZoneId;
    }
    return { date: date, dateTime: toRfc3339(dateTime, "dateTime"), timeZone: zone };
}

function putBool(query: Map<string, string>, name: string, value: boolean | undefined): void {
    if (value !== undefined) query.set(name, value ? "true" : "false");
}

function bounded(value: number | undefined, fallback: number, min: number, max: number): number {
    const actual = value ?? fallback;
    if (actual < min) return min;
    if (actual > max) return max;
    return actual;
}

function eventStart(event: Event): string {
    return event.start.dateTime ?? event.start.date ?? "";
}

function meetRequestId(): string {
    return "submilli-" + Temporal.Now.instant().epochMilliseconds.toString();
}

function minutesBetween(start: string, end: string): number {
    const a = Temporal.Instant.from(start);
    const b = Temporal.Instant.from(end);
    return a.until(b).total({ unit: "minutes" });
}

function addMinutes(value: string, minutes: number): string {
    return Temporal.Instant.from(value).add({ minutes: minutes }).toString();
}
