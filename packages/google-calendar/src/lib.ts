import { get, post, patch, delete, Response } from "submilli:http";
import { encodeComponent, encodeQuery } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const API = "https://www.googleapis.com/calendar/v3";

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
    summary?: string;
    description?: string;
    timeZone?: string;
    accessRole?: string;
    primary?: boolean;
    selected?: boolean;
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
    /** Attendee email address. */
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
    /** Event start (all-day date or timed dateTime). */
    start: EventTime;
    /** Event end, exclusive (all-day date or timed dateTime). */
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
    status?: string;
    htmlLink?: string;
    summary?: string;
    description?: string;
    location?: string;
    start: EventTime;
    end: EventTime;
    attendees?: Attendee[];
    recurrence?: string[];
    recurringEventId?: string;
    visibility?: string;
    hangoutLink?: string;
    created?: string;
    updated?: string;
}

interface EventListResponse {
    items?: ApiEvent[];
    nextPageToken?: string;
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
    busy?: BusyPeriod[];
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
 * @capability submilli/google-calendar.listCalendars {}
 */
export function listCalendars(page: PageOptions | null = null): Page<Calendar> {
    check("submilli/google-calendar.listCalendars", {});
    const query = new Map<string, string>();
    applyPage(query, page, 20, 250);
    const data = calendarGet("/users/me/calendarList", query).json() as CalendarListResponse;
    const items: Calendar[] = [];
    if (data.items !== null) for (const item of data.items) items.push(calendarFrom(item));
    return { items: items, nextPageToken: str(data.nextPageToken) };
}

/**
 * List one page of events. The default calendar is `primary`.
 * @capability submilli/google-calendar.listEvents { calendarId: string }
 */
export function listEvents(options: EventListOptions | null = null): Page<Event> {
    let calendarId = "primary";
    if (options !== null) {
        const requestedCalendar = options.calendarId;
        if (requestedCalendar !== null) calendarId = requestedCalendar;
    }
    check("submilli/google-calendar.listEvents", { calendarId: calendarId });
    const query = new Map<string, string>();
    query.set("maxResults", bounded(options === null ? null : options.limit, 20, 1, 2500).toString());
    if (options !== null) {
        putQuery(query, "pageToken", options.pageToken);
        putTimeQuery(query, "timeMin", options.timeMin);
        putTimeQuery(query, "timeMax", options.timeMax);
        putQuery(query, "q", options.query);
        putBool(query, "singleEvents", options.singleEvents);
        putQuery(query, "orderBy", options.orderBy);
        putBool(query, "showDeleted", options.showDeleted);
        putQuery(query, "timeZone", options.timeZone);
    }
    const data = calendarGet("/calendars/" + encodeComponent(calendarId) + "/events", query).json() as EventListResponse;
    return eventPage(data);
}

/**
 * Fetch one event, returning null when it does not exist.
 * @capability submilli/google-calendar.getEvent { calendarId: string }
 */
export function getEvent(eventId: string, calendarId: string = "primary"): Event | null {
    check("submilli/google-calendar.getEvent", { calendarId: calendarId });
    return fetchEvent(eventId, calendarId);
}

function fetchEvent(eventId: string, calendarId: string): Event | null {
    const response = calendarRawGet("/calendars/" + encodeComponent(calendarId) + "/events/" + encodeComponent(eventId), new Map<string, string>());
    if (response.status === 404) return null;
    requireOk(response);
    return eventFrom(response.json() as ApiEvent);
}

/**
 * Create a Calendar event and optionally request a Google Meet conference.
 * @capability submilli/google-calendar.createEvent { calendarId: string }
 */
export function createEvent(input: EventCreateInput, calendarId: string = "primary"): Event {
    check("submilli/google-calendar.createEvent", { calendarId: calendarId });
    const body: EventCreateBody = { summary: input.summary, start: normalizeEventTime(input.start), end: normalizeEventTime(input.end) };
    if (input.description !== null) body.description = input.description;
    if (input.location !== null) body.location = input.location;
    if (input.attendees !== null) body.attendees = input.attendees;
    if (input.recurrence !== null) body.recurrence = input.recurrence;
    if (input.reminders !== null) body.reminders = input.reminders;
    if (input.visibility !== null) body.visibility = input.visibility;
    const query = new Map<string, string>();
    putQuery(query, "sendUpdates", input.sendUpdates);
    if (input.createGoogleMeet === true) {
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
 * Patch an event. Omitted fields remain unchanged; empty attendees clears attendees.
 * @capability submilli/google-calendar.updateEvent { calendarId: string }
 */
export function updateEvent(eventId: string, input: EventUpdateInput, calendarId: string = "primary"): Event {
    check("submilli/google-calendar.updateEvent", { calendarId: calendarId });
    const body: EventUpdateBody = {};
    if (input.summary !== null) body.summary = input.summary;
    if (input.start !== null) body.start = normalizeEventTime(input.start);
    if (input.end !== null) body.end = normalizeEventTime(input.end);
    if (input.clearDescription === true) {
        body.description = null;
    } else if (input.description !== null) {
        body.description = input.description;
    }
    if (input.clearLocation === true) {
        body.location = null;
    } else if (input.location !== null) {
        body.location = input.location;
    }
    if (input.attendees !== null) body.attendees = input.attendees;
    if (input.recurrence !== null) body.recurrence = input.recurrence;
    if (input.reminders !== null) body.reminders = input.reminders;
    if (input.visibility !== null) body.visibility = input.visibility;
    const query = new Map<string, string>();
    putQuery(query, "sendUpdates", input.sendUpdates);
    const path = "/calendars/" + encodeComponent(calendarId) + "/events/" + encodeComponent(eventId);
    const response = patch(API + calendarPath(path, query), body, authHeaders());
    requireOk(response);
    return eventFrom(response.json() as ApiEvent);
}

/**
 * Set the authenticated attendee's response status on an event.
 * @capability submilli/google-calendar.respondToEvent { calendarId: string, response: string }
 */
export function respondToEvent(eventId: string, response: string, calendarId: string = "primary"): Event {
    check("submilli/google-calendar.respondToEvent", { calendarId: calendarId, response: response });
    if (response !== "accepted" && response !== "declined" && response !== "tentative" && response !== "needsAction") {
        throw new CalendarError("invalid_response", "response must be accepted, declined, tentative, or needsAction", 0);
    }
    const current = fetchEvent(eventId, calendarId);
    if (current === null) throw new CalendarError("not_found", "Calendar event was not found", 404);
    const attendees: Attendee[] = [];
    let foundSelf = false;
    for (const attendee of current.attendees) {
        if (attendee.self === true) {
            attendee.responseStatus = response;
            attendees.push(attendee);
            foundSelf = true;
        } else {
            attendees.push(attendee);
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
 * Delete an event. This is idempotent when the event is already absent.
 * @capability submilli/google-calendar.deleteEvent { calendarId: string }
 */
export function deleteEvent(eventId: string, options: EventDeleteOptions | null = null): void {
    let calendarId = "primary";
    if (options !== null) {
        const requestedCalendar = options.calendarId;
        if (requestedCalendar !== null) calendarId = requestedCalendar;
    }
    check("submilli/google-calendar.deleteEvent", { calendarId: calendarId });
    const query = new Map<string, string>();
    if (options !== null) putQuery(query, "sendUpdates", options.sendUpdates);
    const path = "/calendars/" + encodeComponent(calendarId) + "/events/" + encodeComponent(eventId);
    const response = delete(API + calendarPath(path, query), authHeaders());
    if (response.status !== 404) requireOk(response);
}

/**
 * Query busy intervals for explicit calendars and a bounded time range.
 * @capability submilli/google-calendar.queryFreeBusy { calendarIds: string[] }
 */
export function queryFreeBusy(input: FreeBusyInput): FreeBusyResult {
    const calendarIds = input.calendarIds;
    check("submilli/google-calendar.queryFreeBusy", { calendarIds: calendarIds });
    if (calendarIds.length === 0) throw new CalendarError("invalid_input", "calendarIds must not be empty", 0);
    if (calendarIds.length > 50) throw new CalendarError("too_many_calendars", "free/busy accepts at most 50 calendars", 0);
    const timeMin = toRfc3339(input.timeMin, "timeMin");
    const timeMax = toRfc3339(input.timeMax, "timeMax");
    const calendars: CalendarBusy[] = [];
    for (const id of calendarIds) {
        const body: FreeBusyRequest = {
            timeMin: timeMin,
            timeMax: timeMax,
            items: [{ id: id }],
        };
        if (input.timeZone !== null) body.timeZone = input.timeZone;
        const response = post(API + "/freeBusy", body, authHeaders());
        requireOk(response);
        const data = response.json() as FreeBusyResponse;
        const values = Object.values(data.calendars);
        let busy: BusyPeriod[] = [];
        if (values.length > 0) {
            const value = values[0] as ApiFreeBusyCalendar;
            const periods = value.busy;
            if (periods !== null) busy = periods;
        }
        calendars.push({ calendarId: id, busy: busy });
    }
    return { timeMin: timeMin, timeMax: timeMax, calendars: calendars };
}

/**
 * Find bounded candidate slots by merging busy intervals from multiple calendars.
 * @capability submilli/google-calendar.findFreeTime { calendarIds: string[] }
 */
export function findFreeTime(input: FindFreeTimeInput): TimeSlot[] {
    const calendarIds = input.calendarIds;
    check("submilli/google-calendar.findFreeTime", { calendarIds: calendarIds });
    if (input.durationMinutes <= 0) throw new CalendarError("invalid_duration", "durationMinutes must be positive", 0);
    const timeMin = toRfc3339(input.timeMin, "timeMin");
    const timeMax = toRfc3339(input.timeMax, "timeMax");
    const query: FreeBusyInput = {
        calendarIds: calendarIds,
        timeMin: timeMin,
        timeMax: timeMax,
    };
    if (input.timeZone !== null) query.timeZone = input.timeZone;
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
    const maxSlots = bounded(input.maxSlots, 10, 1, 100);
    const slots: TimeSlot[] = [];
    let cursor = timeMin;
    for (const period of merged) {
        if (slots.length >= maxSlots) break;
        if (period.start > cursor && minutesBetween(cursor, period.start) >= input.durationMinutes) {
            slots.push({ start: cursor, end: addMinutes(cursor, input.durationMinutes) });
        }
        if (period.end > cursor) cursor = period.end;
    }
    if (slots.length < maxSlots && cursor < timeMax && minutesBetween(cursor, timeMax) >= input.durationMinutes) {
        slots.push({ start: cursor, end: addMinutes(cursor, input.durationMinutes) });
    }
    return slots;
}

/**
 * Build a bounded chronological agenda across explicit calendars or up to ten visible calendars.
 * @capability submilli/google-calendar.agenda {}
 */
export function agenda(options: AgendaOptions): AgendaResult {
    check("submilli/google-calendar.agenda", {});
    const calendarLimit = bounded(options.maxCalendars, 10, 1, 10);
    const eventLimit = bounded(options.maxEventsPerCalendar, 20, 1, 100);
    const timeMin = toRfc3339(options.timeMin, "timeMin");
    const timeMax = toRfc3339(options.timeMax, "timeMax");
    const ids: string[] = [];
    if (options.calendarIds !== null) {
        for (const id of options.calendarIds) if (ids.length < calendarLimit) ids.push(id);
    } else {
        const page = listCalendars({ limit: calendarLimit });
        for (const calendar of page.items) ids.push(calendar.id);
    }
    const events: AgendaEvent[] = [];
    let truncated = false;
    const requestedIds = options.calendarIds;
    if (requestedIds !== null) truncated = requestedIds.length > ids.length;
    for (const id of ids) {
        const eventOptions: EventListOptions = {
            calendarId: id,
            limit: eventLimit,
            timeMin: timeMin,
            timeMax: timeMax,
            singleEvents: true,
            orderBy: "startTime",
        };
        if (options.timeZone !== null) eventOptions.timeZone = options.timeZone;
        const page = listEvents(eventOptions);
        for (const event of page.items) events.push({ calendarId: id, event: event });
        if (page.nextPageToken.length > 0) truncated = true;
    }
    events.sort((a: AgendaEvent, b: AgendaEvent): number => eventStart(a.event) < eventStart(b.event) ? -1 : eventStart(a.event) > eventStart(b.event) ? 1 : 0);
    return { events: events, truncated: truncated };
}

function eventPage(data: EventListResponse): Page<Event> {
    const items: Event[] = [];
    if (data.items !== null) for (const item of data.items) items.push(eventFrom(item));
    return { items: items, nextPageToken: str(data.nextPageToken) };
}

function eventFrom(item: ApiEvent): Event {
    return {
        id: item.id,
        status: str(item.status),
        htmlLink: str(item.htmlLink),
        summary: str(item.summary),
        description: str(item.description),
        location: str(item.location),
        start: item.start,
        end: item.end,
        attendees: item.attendees !== null ? item.attendees : [],
        recurrence: item.recurrence !== null ? item.recurrence : [],
        recurringEventId: str(item.recurringEventId),
        visibility: str(item.visibility),
        hangoutLink: str(item.hangoutLink),
        created: str(item.created),
        updated: str(item.updated),
    };
}

function calendarFrom(item: ApiCalendar): Calendar {
    return {
        id: item.id,
        summary: str(item.summary),
        description: str(item.description),
        timeZone: str(item.timeZone),
        accessRole: str(item.accessRole),
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
    if (token === null) throw new CalendarError("missing_token", "GOOGLE_ACCESS_TOKEN is not bound", 0);
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
        if (body !== null) {
            const errors = body.errors;
            const bodyStatus = body.status;
            const bodyMessage = body.message;
            if (errors !== null && errors.length > 0) {
                const reason = errors[0].reason;
                if (reason !== null) code = reason;
            } else if (bodyStatus !== null) {
                code = bodyStatus;
            }
            if (bodyMessage !== null) message = bodyMessage;
        }
    }
    throw new CalendarError(code, message, response.status);
}

function applyPage(query: Map<string, string>, page: PageOptions | null, defaultLimit: number, maxLimit: number): void {
    query.set("maxResults", bounded(page === null ? null : page.limit, defaultLimit, 1, maxLimit).toString());
    if (page !== null) putQuery(query, "pageToken", page.pageToken);
}

function putQuery(query: Map<string, string>, name: string, value: string | null): void {
    if (value !== null) query.set(name, value);
}

function putTimeQuery(query: Map<string, string>, name: string, value: string | null): void {
    if (value !== null) query.set(name, toRfc3339(value, name));
}

function toRfc3339(value: string, param: string): string {
    try {
        return Temporal.Instant.from(value).toString();
    } catch (e) {
        throw new CalendarError("invalid_timestamp", param + " is not an RFC 3339 timestamp: " + (e as Error).message, 0);
    }
}

function normalizeEventTime(time: EventTime): EventTime {
    const dateTime = time.dateTime;
    if (dateTime === null) return time;
    const normalized: EventTime = { dateTime: toRfc3339(dateTime, "dateTime") };
    if (time.date !== null) normalized.date = time.date;
    let zone = time.timeZone;
    if (zone === null && dateTime.indexOf("[") >= 0) {
        zone = Temporal.ZonedDateTime.from(dateTime).timeZoneId;
    }
    if (zone !== null) normalized.timeZone = zone;
    return normalized;
}

function putBool(query: Map<string, string>, name: string, value: boolean | null): void {
    if (value !== null) query.set(name, value ? "true" : "false");
}

function bounded(value: number | null, fallback: number, min: number, max: number): number {
    const actual = value === null ? fallback : value;
    if (actual < min) return min;
    if (actual > max) return max;
    return actual;
}

function str(value: string | null): string {
    return value === null ? "" : value;
}

function eventStart(event: Event): string {
    if (event.start.dateTime !== null) return event.start.dateTime;
    return str(event.start.date);
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
