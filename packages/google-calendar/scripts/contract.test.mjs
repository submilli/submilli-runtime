import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage, nullableFields } from '../../../scripts/package-contract-host.mjs';
const host = contractHost();
const calendar = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);
const options = nullableFields;
const event = { id: 'event', attendees: [{ email: 'REMOVED@Example.Com.' }, { email: 'kept@example.com' }, {}] };

test('notification policies include removed attendees for all notification modes', () => {
    for (const sendUpdates of ['all', 'externalOnly']) {
        host.reset(); host.response(() => ({ data: event }));
        host.denial((cap, ctx) => ctx.notificationRecipients.includes('removed@example.com'));
        assert.throws(() => calendar.updateEvent('event', options({ attendees: [{ email: 'kept@example.com' }, { email: 'new@example.com' }], sendUpdates })), /Capability denied/);
        assert.deepEqual(host.checks[0].context, { calendarId: 'primary', attendees: ['kept@example.com', 'new@example.com'], removedAttendees: ['removed@example.com'], notificationRecipients: ['removed@example.com', 'kept@example.com', 'new@example.com'], sendUpdates });
        assert.equal(host.requests.length, 1); assert.equal(host.requests[0].method, 'get');
        assert.equal(new URL(host.requests[0].url).searchParams.get('fields'), 'attendees(email),attendeesOmitted');
    }
});

test('none skips metadata for explicit replacements; omitted attendees are read once', () => {
    host.reset(); host.response(() => ({ data: event }));
    calendar.updateEvent('event', options({ attendees: [], sendUpdates: 'none' }));
    assert.equal(host.requests.length, 1); assert.equal(host.requests[0].method, 'patch');
    assert.deepEqual(host.checks[0].context.notificationRecipients, []);
    host.reset(); calendar.updateEvent('event', options({ summary: 'Renamed', sendUpdates: 'all' }));
    assert.deepEqual(host.requests.map((r) => r.method), ['get', 'patch']);
    assert.deepEqual(host.checks[0].context.attendees, ['removed@example.com', 'kept@example.com']);
    assert.deepEqual(host.checks[0].context.removedAttendees, []);
    assert.deepEqual(host.checks[0].context.notificationRecipients, ['removed@example.com', 'kept@example.com']);
    assert.ok(!Object.hasOwn(host.requests[1].body, 'attendees'));
});

test('clearing attendees checks prior recipients and missing events cannot mutate', () => {
    host.reset(); host.response(() => ({ data: event }));
    calendar.updateEvent('event', options({ attendees: [], sendUpdates: 'all' }));
    assert.deepEqual(host.checks[0].context.removedAttendees, ['removed@example.com', 'kept@example.com']);
    assert.deepEqual(host.requests[1].body.attendees, []);
    host.reset(); host.response(() => ({ status: 404, data: {} }));
    assert.throws(() => calendar.updateEvent('missing', options({ attendees: [], sendUpdates: 'all' })), (e) => e instanceof calendar.CalendarError && e.code === 'not_found');
    assert.equal(host.requests.length, 1); assert.equal(host.checks.length, 0);
});

test('an incomplete attendee list cannot authorize notifications', () => {
    host.reset(); host.response(() => ({ data: { ...event, attendeesOmitted: true } }));
    assert.throws(() => calendar.updateEvent('event', options({ attendees: [], sendUpdates: 'all' })), (e) => e.code === 'incomplete_attendees');
    assert.equal(host.requests.length, 1); assert.equal(host.checks.length, 0);
});
