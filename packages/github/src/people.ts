// Response mapping for repository audience and public profile reads. Internal: `src/lib.ts`
// re-exports the result types and calls these mappers. They live here so `tests/lib.test.ts` can
// run them in the real runtime: `submilli build test` has no HTTP mock.

/** One entry of a repository's public activity feed. */
export interface RepositoryEvent {
    /** GitHub's event id. */
    id: string;
    /** Event type, e.g. "WatchEvent" for a star, "ForkEvent", "PushEvent". */
    type: string;
    /** Login of the account that acted. */
    actorLogin: string;
    /** ISO 8601 creation time. */
    createdAt: string;
    /** What happened within the type, e.g. "started" on a WatchEvent; "" when the event has no action. */
    action: string;
    /** "owner/name" of the new fork on a ForkEvent; "" on every other type. */
    forkFullName: string;
}

/** One account that starred a repository. */
export interface Stargazer {
    /** Login; "" when GitHub no longer links the star to an account. */
    login: string;
    /** ISO 8601 time of the star. */
    starredAt: string;
    /** Account type, e.g. "User", "Organization", or "Bot". */
    accountType: string;
}

/** One fork of a repository. */
export interface RepositoryFork {
    /** "owner/name" form. */
    fullName: string;
    /** Login of the fork's owner. */
    ownerLogin: string;
    /** ISO 8601 creation time. */
    createdAt: string;
    /** Web URL of the fork. */
    url: string;
}

/** An account's public GitHub profile. A field the account left unset is "". */
export interface UserProfile {
    /** Login (username). */
    login: string;
    /** Display name. */
    name: string;
    /** Company, as the account wrote it. */
    company: string;
    /** Biography. */
    bio: string;
    /** Website, as the account wrote it. */
    blog: string;
    /** Twitter/X username. */
    twitterUsername: string;
    /** Location, as the account wrote it. */
    location: string;
    /** Web URL of the profile. */
    url: string;
    /** Account type, e.g. "User", "Organization", or "Bot". */
    accountType: string;
    /** Follower count. */
    followers: number;
    /** Count of accounts this one follows. */
    following: number;
    /** Public repository count. */
    publicRepos: number;
    /** ISO 8601 account creation time. */
    createdAt: string;
    /** True when the profile shows a public email address. The address itself is never returned. */
    hasPublicEmail: boolean;
}

/** One organization an account publicly belongs to. */
export interface UserOrganization {
    /** Organization login. */
    login: string;
    /** Web URL of the organization. */
    url: string;
}

/** One social account linked from a GitHub profile. */
export interface UserSocialAccount {
    /** Provider name, e.g. "twitter", "linkedin", or "generic". */
    provider: string;
    /** URL of the account. */
    url: string;
}

export interface ApiAccount {
    login?: string | null;
    type?: string | null;
}

export interface ApiForkee {
    full_name?: string | null;
}

export interface ApiEventPayload {
    action?: string | null;
    forkee?: ApiForkee | null;
}

export interface ApiEvent {
    // GitHub documents this as an integer and sends a string; casts are checked, so accept both.
    id?: string | number | null;
    type?: string | null;
    actor?: ApiAccount | null;
    created_at?: string | null;
    payload?: ApiEventPayload | null;
}

export interface ApiStargazer {
    starred_at?: string | null;
    user?: ApiAccount | null;
}

export interface ApiFork {
    full_name?: string | null;
    html_url?: string | null;
    created_at?: string | null;
    owner?: ApiAccount | null;
}

// `email` is declared only to be reduced to `hasPublicEmail`. Nothing else may read it.
export interface ApiUserProfile {
    login?: string | null;
    name?: string | null;
    company?: string | null;
    bio?: string | null;
    blog?: string | null;
    twitter_username?: string | null;
    location?: string | null;
    html_url?: string | null;
    type?: string | null;
    followers?: number | null;
    following?: number | null;
    public_repos?: number | null;
    created_at?: string | null;
    email?: string | null;
}

export interface ApiUserOrganization {
    login?: string | null;
}

export interface ApiUserSocialAccount {
    provider?: string | null;
    url?: string | null;
}

export function repositoryEventFrom(data: ApiEvent): RepositoryEvent {
    const type = data.type ?? "";
    return {
        id: eventId(data.id),
        type: type,
        actorLogin: data.actor?.login ?? "",
        createdAt: data.created_at ?? "",
        action: data.payload?.action ?? "",
        forkFullName: type === "ForkEvent" ? (data.payload?.forkee?.full_name ?? "") : "",
    };
}

function eventId(id?: string | number | null): string {
    if (id === undefined || id === null) return "";
    return typeof id === "string" ? id : id.toString();
}

export function stargazerFrom(data: ApiStargazer): Stargazer {
    return {
        login: data.user?.login ?? "",
        starredAt: data.starred_at ?? "",
        accountType: data.user?.type ?? "",
    };
}

export function repositoryForkFrom(data: ApiFork): RepositoryFork {
    return {
        fullName: data.full_name ?? "",
        ownerLogin: data.owner?.login ?? "",
        createdAt: data.created_at ?? "",
        url: data.html_url ?? "",
    };
}

export function userProfileFrom(data: ApiUserProfile): UserProfile {
    return {
        login: data.login ?? "",
        name: data.name ?? "",
        company: data.company ?? "",
        bio: data.bio ?? "",
        blog: data.blog ?? "",
        twitterUsername: data.twitter_username ?? "",
        location: data.location ?? "",
        url: data.html_url ?? "",
        accountType: data.type ?? "",
        followers: data.followers ?? 0,
        following: data.following ?? 0,
        publicRepos: data.public_repos ?? 0,
        createdAt: data.created_at ?? "",
        hasPublicEmail: (data.email ?? "").length > 0,
    };
}

// GitHub's organization listing carries only API URLs, so the web URL is derived from the login,
// matching `url` on every other result here.
export function userOrganizationFrom(data: ApiUserOrganization): UserOrganization {
    const login = data.login ?? "";
    return { login: login, url: login.length === 0 ? "" : "https://github.com/" + login };
}

export function userSocialAccountFrom(data: ApiUserSocialAccount): UserSocialAccount {
    return { provider: data.provider ?? "", url: data.url ?? "" };
}
