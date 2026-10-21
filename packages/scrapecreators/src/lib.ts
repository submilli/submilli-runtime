/** One provider-reported partial failure or warning; raw retains its full detail. */
export interface ProviderIssue {
    /** Whether the provider reported an error or warning. */
    kind: "error" | "warning";
    /** Provider code, when available. */
    code: string | number | null;
    /** Affected source URL, when available. */
    url: string | null;
    /** Provider message or a JSON rendering of the issue. */
    message: string;
    /** Original issue value. */
    raw: unknown;
}
/** ScrapeCreators public-data clients. Response data keeps provider field names and units. */
export interface Result<T> {
    /** Typed provider response, including native pagination and credit fields. */
    data: T;
    /** Entire provider JSON, including future fields and partial-failure details. */
    raw: unknown;
    /** Provider-reported errors/warnings, including failures inside HTTP 200. */
    issues: ProviderIssue[];
}
/** Options for youtube.getChannel. */
export interface YouTubeGetChannelOptions {
    /** Provider option channelId. See the operation reference for values and defaults. */
    channelId?: string;
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option url. See the operation reference for values and defaults. */
    url?: string;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native youtube getChannel fields; optional metadata may be missing or null. */
export interface YouTubeGetChannelData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider channelId field; see the operation reference for its format. */
    "channelId": string | null;
    /** Native provider channel field; see the operation reference for its format. */
    "channel"?: string | null;
    /** Native provider name field; see the operation reference for its format. */
    "name"?: string | null;
    /** Native provider avatar field; see the operation reference for its format. */
    "avatar"?: {
        "image"?: {
            "sources"?: ({
                "url"?: string | null;
                "width"?: number | null;
                "height"?: number | null;
            })[] | null;
            "processor"?: {
                "borderImageProcessor"?: unknown;
            } | null;
        } | null;
        "avatarImageSize"?: string | null;
        "loggingDirectives"?: {
            "trackingParams"?: string | null;
            "visibility"?: {
                "types"?: string | null;
            } | null;
        } | null;
    } | null;
    /** Native provider description field; see the operation reference for its format. */
    "description"?: string | null;
    /** Native provider subscriberCount field; see the operation reference for its format. */
    "subscriberCount"?: number | null;
    /** Native provider subscriberCountText field; see the operation reference for its format. */
    "subscriberCountText"?: string | null;
    /** Native provider videoCountText field; see the operation reference for its format. */
    "videoCountText"?: string | null;
    /** Native provider viewCountText field; see the operation reference for its format. */
    "viewCountText"?: string | null;
    /** Native provider joinedDateText field; see the operation reference for its format. */
    "joinedDateText"?: string | null;
    /** Native provider tags field; see the operation reference for its format. */
    "tags"?: string | null;
    /** Native provider email field; see the operation reference for its format. */
    "email"?: string | null;
    /** Native provider store field; see the operation reference for its format. */
    "store"?: string | null;
    /** Native provider twitter field; see the operation reference for its format. */
    "twitter"?: string | null;
    /** Native provider instagram field; see the operation reference for its format. */
    "instagram"?: string | null;
    /** Native provider links field; see the operation reference for its format. */
    "links"?: (string)[] | null;
    /** Native provider country field; see the operation reference for its format. */
    "country"?: string | null;
}
/** Options for youtube.listChannelVideos. */
export interface YouTubeListChannelVideosOptions {
    /** Provider option channelId. See the operation reference for values and defaults. */
    channelId?: string;
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option sort. See the operation reference for values and defaults. */
    sort?: "latest" | "popular";
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
    /** Provider option isPaidPromotions. See the operation reference for values and defaults. */
    isPaidPromotions?: boolean;
    /** Provider option includeExtras. See the operation reference for values and defaults. */
    includeExtras?: boolean;
}
/** Native youtube listChannelVideos fields; optional metadata may be missing or null. */
export interface YouTubeListChannelVideosData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider videos field; see the operation reference for its format. */
    "videos": ({
        "type"?: string | null;
        "id"?: string | null;
        "url"?: string | null;
        "title"?: string | null;
        "description"?: string | null;
        "thumbnail"?: string | null;
        "channel"?: {
            "title"?: string | null;
            "thumbnail"?: string | null;
        } | null;
        "viewCountText"?: string | null;
        "viewCountInt"?: number | null;
        "publishedTimeText"?: string | null;
        "publishedTime"?: string | null;
        "lengthText"?: string | null;
        "lengthSeconds"?: number | null;
        "badges"?: unknown[] | null;
    })[];
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.listChannelPlaylists. */
export interface YouTubeListChannelPlaylistsOptions {
    /** Provider option channelId. See the operation reference for values and defaults. */
    channelId?: string;
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
}
/** Native youtube listChannelPlaylists fields; optional metadata may be missing or null. */
export interface YouTubeListChannelPlaylistsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider playlists field; see the operation reference for its format. */
    "playlists": ({
        "type"?: string | null;
        "id"?: string | null;
        "title"?: string | null;
        "thumbnail"?: string | null;
        "playlistUrl"?: string | null;
        "videoId"?: string | null;
        "videoUrl"?: string | null;
        "videoCount"?: number | null;
        "channel"?: {
            "id"?: string | null;
            "title"?: string | null;
            "handle"?: string | null;
            "channelUrl"?: string | null;
        } | null;
    })[];
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.listChannelLives. */
export interface YouTubeListChannelLivesOptions {
    /** Provider option channelId. See the operation reference for values and defaults. */
    channelId?: string;
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
}
/** Native youtube listChannelLives fields; optional metadata may be missing or null. */
export interface YouTubeListChannelLivesData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider lives field; see the operation reference for its format. */
    "lives": ({
        "type"?: string | null;
        "id"?: string | null;
        "url"?: string | null;
        "title"?: string | null;
        "thumbnail"?: string | null;
        "channel"?: {
            "id"?: string | null;
            "title"?: string | null;
            "handle"?: string | null;
            "thumbnail"?: string | null;
        } | null;
        "viewCountText"?: string | null;
        "viewCountInt"?: number | null;
        "publishedTimeText"?: string | null;
        "lengthText"?: string | null;
        "lengthSeconds"?: number | null;
        "badges"?: unknown[] | null;
    })[];
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.listChannelCommunityPosts. */
export interface YouTubeListChannelCommunityPostsOptions {
    /** Provider option channelId. See the operation reference for values and defaults. */
    channelId?: string;
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
}
/** Native youtube listChannelCommunityPosts fields; optional metadata may be missing or null. */
export interface YouTubeListChannelCommunityPostsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider posts field; see the operation reference for its format. */
    "posts": ({
        "id"?: string | null;
        "url"?: string | null;
        "channel"?: {
            "id"?: string | null;
            "title"?: string | null;
            "url"?: string | null;
            "handle"?: string | null;
        } | null;
        "content"?: string | null;
        "image"?: string | null;
        "images"?: (string)[] | null;
        "likeCountText"?: string | null;
        "likeCount"?: number | null;
        "publishedTimeText"?: string | null;
        "publishedTime"?: string | null;
        "video"?: {
            "id"?: string | null;
            "title"?: string | null;
            "thumbnail"?: string | null;
            "url"?: string | null;
            "viewCountText"?: string | null;
            "viewCountInt"?: number | null;
            "publishedTimeText"?: string | null;
            "lengthText"?: string | null;
            "lengthSeconds"?: number | null;
        } | null;
    })[];
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.listChannelShorts. */
export interface YouTubeListChannelShortsOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option channelId. See the operation reference for values and defaults. */
    channelId?: string;
    /** Provider option sort. See the operation reference for values and defaults. */
    sort?: "newest" | "popular";
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
}
/** Native youtube listChannelShorts fields; optional metadata may be missing or null. */
export interface YouTubeListChannelShortsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider videos field; see the operation reference for its format. */
    "videos"?: unknown[] | null;
    /** Native provider channels field; see the operation reference for its format. */
    "channels"?: unknown[] | null;
    /** Native provider playlists field; see the operation reference for its format. */
    "playlists"?: unknown[] | null;
    /** Native provider shorts field; see the operation reference for its format. */
    "shorts": ({
        "type"?: string | null;
        "id"?: string | null;
        "url"?: string | null;
        "title"?: string | null;
        "thumbnail"?: string | null;
        "viewCountText"?: string | null;
        "viewCountInt"?: number | null;
        "description"?: string | null;
        "commentCountText"?: string | null;
        "commentCountInt"?: number | null;
        "likeCountInt"?: number | null;
        "likeCountText"?: string | null;
        "publishDate"?: string | null;
        "genre"?: string | null;
        "durationMs"?: number | null;
        "durationFormatted"?: string | null;
    })[];
    /** Native provider shelves field; see the operation reference for its format. */
    "shelves"?: unknown[] | null;
    /** Native provider lives field; see the operation reference for its format. */
    "lives"?: unknown[] | null;
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.getVideo. */
export interface YouTubeGetVideoOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option language. See the operation reference for values and defaults. */
    language?: string;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native youtube getVideo fields; optional metadata may be missing or null. */
export interface YouTubeGetVideoData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider id field; see the operation reference for its format. */
    "id": string | null;
    /** Native provider thumbnail field; see the operation reference for its format. */
    "thumbnail"?: string | null;
    /** Native provider url field; see the operation reference for its format. */
    "url"?: string | null;
    /** Native provider publishDate field; see the operation reference for its format. */
    "publishDate"?: string | null;
    /** Native provider type field; see the operation reference for its format. */
    "type"?: string | null;
    /** Native provider title field; see the operation reference for its format. */
    "title"?: string | null;
    /** Native provider description field; see the operation reference for its format. */
    "description"?: string | null;
    /** Native provider descriptionLinks field; see the operation reference for its format. */
    "descriptionLinks"?: unknown[] | null;
    /** Native provider commentCountText field; see the operation reference for its format. */
    "commentCountText"?: string | null;
    /** Native provider commentCountInt field; see the operation reference for its format. */
    "commentCountInt"?: number | null;
    /** Native provider likeCountText field; see the operation reference for its format. */
    "likeCountText"?: string | null;
    /** Native provider likeCountInt field; see the operation reference for its format. */
    "likeCountInt"?: number | null;
    /** Native provider viewCountText field; see the operation reference for its format. */
    "viewCountText"?: string | null;
    /** Native provider viewCountInt field; see the operation reference for its format. */
    "viewCountInt"?: number | null;
    /** Native provider publishDateText field; see the operation reference for its format. */
    "publishDateText"?: string | null;
    /** Native provider collaborators field; see the operation reference for its format. */
    "collaborators"?: unknown[] | null;
    /** Native provider channel field; see the operation reference for its format. */
    "channel"?: {
        "id"?: string | null;
        "url"?: string | null;
        "handle"?: string | null;
        "title"?: string | null;
    } | null;
    /** Native provider chapters field; see the operation reference for its format. */
    "chapters"?: unknown[] | null;
    /** Native provider watchNextVideos field; see the operation reference for its format. */
    "watchNextVideos"?: ({
        "id"?: string | null;
        "title"?: string | null;
        "thumbnail"?: string | null;
        "channel"?: {
            "title"?: string | null;
            "url"?: string | null;
            "handle"?: string | null;
            "id"?: string | null;
        } | null;
        "publishedTimeText"?: string | null;
        "publishedTime"?: string | null;
        "publishDateText"?: string | null;
        "publishDate"?: string | null;
        "viewCountText"?: string | null;
        "viewCountInt"?: number | null;
        "lengthText"?: string | null;
        "lengthInSeconds"?: number | null;
        "videoUrl"?: string | null;
    })[] | null;
    /** Native provider keywords field; see the operation reference for its format. */
    "keywords"?: unknown[] | null;
    /** Native provider genre field; see the operation reference for its format. */
    "genre"?: string | null;
    /** Native provider durationMs field; see the operation reference for its format. */
    "durationMs"?: number | null;
    /** Native provider durationFormatted field; see the operation reference for its format. */
    "durationFormatted"?: string | null;
    /** Native provider isPaidPromotion field; see the operation reference for its format. */
    "isPaidPromotion"?: boolean | null;
    /** Native provider most_replayed field; see the operation reference for its format. */
    "most_replayed"?: unknown;
    /** Native provider captionTracks field; see the operation reference for its format. */
    "captionTracks"?: ({
        "baseUrl"?: string | null;
        "name"?: {
            "simpleText"?: string | null;
        } | null;
        "vssId"?: string | null;
        "languageCode"?: string | null;
        "kind"?: string | null;
        "isTranslatable"?: boolean | null;
        "trackName"?: string | null;
    })[] | null;
}
/** Options for youtube.getTranscript. */
export interface YouTubeGetTranscriptOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option language. See the operation reference for values and defaults. */
    language?: string;
    /** Provider option originalAudio. See the operation reference for values and defaults. */
    originalAudio?: boolean;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native youtube getTranscript fields; optional metadata may be missing or null. */
export interface YouTubeGetTranscriptData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider videoId field; see the operation reference for its format. */
    "videoId"?: string | null;
    /** Native provider type field; see the operation reference for its format. */
    "type"?: string | null;
    /** Native provider url field; see the operation reference for its format. */
    "url"?: string | null;
    /** Native transcript text or timestamped segments; null indicates unavailable captions. */
    "transcript": ({
        "text"?: string | null;
        "startMs"?: string | null;
        "endMs"?: string | null;
        "startTimeText"?: string | null;
    })[] | null;
    /** Native provider transcript_only_text field; see the operation reference for its format. */
    "transcript_only_text"?: string | null;
    /** Native provider language field; see the operation reference for its format. */
    "language"?: string | null;
}
/** Options for youtube.getSponsors. */
export interface YouTubeGetSponsorsOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option language. See the operation reference for values and defaults. */
    language?: string;
}
/** Native youtube getSponsors fields; optional metadata may be missing or null. */
export interface YouTubeGetSponsorsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider video field; see the operation reference for its format. */
    "video": {
        "id"?: string | null;
        "url"?: string | null;
        "title"?: string | null;
        "channel"?: {
            "id"?: string | null;
            "url"?: string | null;
            "handle"?: string | null;
            "title"?: string | null;
        } | null;
        "isPaidPromotion"?: boolean | null;
    } | null;
    /** Native provider suspectedSponsors field; see the operation reference for its format. */
    "suspectedSponsors"?: ({
        "name"?: string | null;
        "website"?: string | null;
        "confidence"?: string | null;
        "evidence"?: ({
            "source"?: string | null;
            "text"?: string | null;
        })[] | null;
    })[] | null;
    /** Native provider sponsorDetection field; see the operation reference for its format. */
    "sponsorDetection"?: {
        "status"?: string | null;
        "methods"?: (string)[] | null;
        "transcriptAvailable"?: boolean | null;
        "note"?: string | null;
    } | null;
}
/** Options for youtube.search. */
export interface YouTubeSearchOptions {
    /** Provider option query. See the operation reference for values and defaults. */
    query: string;
    /** Provider option uploadDate. See the operation reference for values and defaults. */
    uploadDate?: "today" | "this_week" | "this_month" | "this_year";
    /** Provider option sortBy. See the operation reference for values and defaults. */
    sortBy?: "relevance" | "popular";
    /** Provider option type. See the operation reference for values and defaults. */
    type?: "videos" | "shorts" | "channels" | "playlists";
    /** Provider option duration. See the operation reference for values and defaults. */
    duration?: "under_3_min" | "between_3_and_20_min" | "over_20_min";
    /** Provider option region. See the operation reference for values and defaults. */
    region?: string;
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
    /** Provider option includeExtras. See the operation reference for values and defaults. */
    includeExtras?: boolean;
}
/** Native youtube search fields; optional metadata may be missing or null. */
export interface YouTubeSearchData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider videos field; see the operation reference for its format. */
    "videos": ({
        "type"?: string | null;
        "id"?: string | null;
        "url"?: string | null;
        "title"?: string | null;
        "thumbnail"?: string | null;
        "channel"?: {
            "id"?: string | null;
            "title"?: string | null;
            "handle"?: string | null;
            "thumbnail"?: string | null;
        } | null;
        "viewCountText"?: string | null;
        "viewCountInt"?: number | null;
        "publishedTimeText"?: string | null;
        "publishedTime"?: string | null;
        "lengthText"?: string | null;
        "lengthSeconds"?: number | null;
        "badges"?: unknown[] | null;
    })[];
    /** Native provider channels field; see the operation reference for its format. */
    "channels"?: unknown[] | null;
    /** Native provider playlists field; see the operation reference for its format. */
    "playlists"?: unknown[] | null;
    /** Native provider shorts field; see the operation reference for its format. */
    "shorts"?: ({
        "type"?: string | null;
        "id"?: string | null;
        "url"?: string | null;
        "title"?: string | null;
        "thumbnail"?: string | null;
        "channel"?: {
            "id"?: string | null;
            "title"?: string | null;
            "handle"?: string | null;
            "thumbnail"?: string | null;
        } | null;
        "viewCountText"?: string | null;
        "viewCountInt"?: number | null;
        "publishedTimeText"?: string | null;
        "publishedTime"?: string | null;
        "lengthText"?: string | null;
        "lengthSeconds"?: number | null;
        "badges"?: unknown[] | null;
    })[] | null;
    /** Native provider shelves field; see the operation reference for its format. */
    "shelves"?: ({
        "type"?: string | null;
        "title"?: string | null;
        "items"?: ({
            "type"?: string | null;
            "id"?: string | null;
            "url"?: string | null;
            "title"?: string | null;
            "thumbnail"?: string | null;
            "viewCountText"?: string | null;
            "viewCountInt"?: number | null;
        })[] | null;
    })[] | null;
    /** Native provider lives field; see the operation reference for its format. */
    "lives"?: unknown[] | null;
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.searchTypeahead. */
export interface YouTubeSearchTypeaheadOptions {
    /** Provider option query. See the operation reference for values and defaults. */
    query: string;
}
/** Native youtube searchTypeahead fields; optional metadata may be missing or null. */
export interface YouTubeSearchTypeaheadData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider data field; see the operation reference for its format. */
    "data": {
        "query"?: string | null;
        "suggestions"?: ({
            "text"?: string | null;
            "type"?: string | null;
            "channel"?: {
                "id"?: string | null;
                "handle"?: string | null;
                "name"?: string | null;
                "thumbnailUrl"?: string | null;
            } | null;
        })[] | null;
    } | null;
}
/** Options for youtube.searchHashtag. */
export interface YouTubeSearchHashtagOptions {
    /** Provider option hashtag. See the operation reference for values and defaults. */
    hashtag: string;
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
    /** Provider option type. See the operation reference for values and defaults. */
    type?: "all" | "shorts";
}
/** Native youtube searchHashtag fields; optional metadata may be missing or null. */
export interface YouTubeSearchHashtagData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider videos field; see the operation reference for its format. */
    "videos": ({
        "type"?: string | null;
        "id"?: string | null;
        "url"?: string | null;
        "title"?: string | null;
        "description"?: string | null;
        "thumbnail"?: string | null;
        "channel"?: {
            "id"?: string | null;
            "title"?: string | null;
            "thumbnail"?: string | null;
        } | null;
        "viewCountText"?: string | null;
        "viewCountInt"?: number | null;
        "publishedTimeText"?: string | null;
        "publishedTime"?: string | null;
        "lengthText"?: string | null;
        "lengthSeconds"?: number | null;
        "badges"?: unknown[] | null;
    })[];
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.listComments. */
export interface YouTubeListCommentsOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
    /** Provider option order. See the operation reference for values and defaults. */
    order?: "top" | "newest";
}
/** Native youtube listComments fields; optional metadata may be missing or null. */
export interface YouTubeListCommentsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider comments field; see the operation reference for its format. */
    "comments": ({
        "id"?: string | null;
        "content"?: string | null;
        "publishedTimeText"?: string | null;
        "publishedTime"?: string | null;
        "replyLevel"?: number | null;
        "author"?: {
            "name"?: string | null;
            "channelId"?: string | null;
            "isVerified"?: boolean | null;
            "isCreator"?: boolean | null;
            "avatarUrl"?: string | null;
            "channelUrl"?: string | null;
        } | null;
        "engagement"?: {
            "likes"?: number | null;
            "replies"?: number | null;
        } | null;
        "repliesContinuationToken"?: string | null;
    })[];
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.listCommentReplies. */
export interface YouTubeListCommentRepliesOptions {
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken: string;
}
/** Native youtube listCommentReplies fields; optional metadata may be missing or null. */
export interface YouTubeListCommentRepliesData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider comments field; see the operation reference for its format. */
    "comments": ({
        "id"?: string | null;
        "content"?: string | null;
        "publishedTimeText"?: string | null;
        "publishedTime"?: string | null;
        "replyLevel"?: number | null;
        "author"?: {
            "name"?: string | null;
            "channelId"?: string | null;
            "isVerified"?: boolean | null;
            "isCreator"?: boolean | null;
            "avatarUrl"?: string | null;
            "channelUrl"?: string | null;
        } | null;
        "engagement"?: {
            "likes"?: number | null;
            "replies"?: number | null;
        } | null;
    })[];
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: string | null;
}
/** Options for youtube.listTrendingShorts. */
export interface YouTubeListTrendingShortsOptions {
}
/** Native youtube listTrendingShorts fields; optional metadata may be missing or null. */
export interface YouTubeListTrendingShortsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider shorts field; see the operation reference for its format. */
    "shorts": ({
        "id"?: string | null;
        "thumbnail"?: string | null;
        "url"?: string | null;
        "title"?: string | null;
        "description"?: string | null;
        "commentCountText"?: string | null;
        "commentCountInt"?: number | null;
        "likeCountText"?: string | null;
        "likeCountInt"?: number | null;
        "viewCountText"?: string | null;
        "viewCountInt"?: number | null;
        "publishDateText"?: string | null;
        "publishDate"?: string | null;
        "channel"?: {
            "id"?: string | null;
            "url"?: string | null;
            "handle"?: string | null;
            "title"?: string | null;
        } | null;
        "chapters"?: unknown[] | null;
        "keywords"?: (string)[] | null;
        "durationMs"?: number | null;
        "durationFormatted"?: string | null;
    })[];
}
/** Options for youtube.getPlaylist. */
export interface YouTubeGetPlaylistOptions {
    /** Provider option playlistId. See the operation reference for values and defaults. */
    playlistId: string;
    /** Provider option continuationToken. See the operation reference for values and defaults. */
    continuationToken?: string;
}
/** Native youtube getPlaylist fields; optional metadata may be missing or null. */
export interface YouTubeGetPlaylistData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider title field; see the operation reference for its format. */
    "title"?: string | null;
    /** Native provider owner field; see the operation reference for its format. */
    "owner"?: {
        "id"?: string | null;
        "name"?: string | null;
        "url"?: string | null;
        "handle"?: string | null;
    } | null;
    /** Native provider totalVideos field; see the operation reference for its format. */
    "totalVideos"?: number | null;
    /** Native provider videos field; see the operation reference for its format. */
    "videos": ({
        "id"?: string | null;
        "title"?: string | null;
        "thumbnail"?: string | null;
        "url"?: string | null;
        "lengthText"?: string | null;
        "lengthSeconds"?: number | null;
        "channel"?: {
            "title"?: string | null;
            "url"?: string | null;
        } | null;
    })[];
    /** Opaque next-page token; pass back unchanged. */
    "continuationToken"?: unknown;
}
/** Options for youtube.getCommunityPost. */
export interface YouTubeGetCommunityPostOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native youtube getCommunityPost fields; optional metadata may be missing or null. */
export interface YouTubeGetCommunityPostData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider id field; see the operation reference for its format. */
    "id": string | null;
    /** Native provider channel field; see the operation reference for its format. */
    "channel"?: {
        "id"?: string | null;
        "title"?: string | null;
        "url"?: string | null;
        "handle"?: string | null;
    } | null;
    /** Native provider content field; see the operation reference for its format. */
    "content"?: string | null;
    /** Native provider images field; see the operation reference for its format. */
    "images"?: (string)[] | null;
    /** Native provider likeCount field; see the operation reference for its format. */
    "likeCount"?: number | null;
    /** Native provider publishedTimeText field; see the operation reference for its format. */
    "publishedTimeText"?: string | null;
    /** Native provider publishedTime field; see the operation reference for its format. */
    "publishedTime"?: string | null;
    /** Native provider video field; see the operation reference for its format. */
    "video"?: unknown;
}
/** Options for linkedin.getProfile. */
export interface LinkedInGetProfileOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native linkedin getProfile fields; optional metadata may be missing or null. */
export interface LinkedInGetProfileData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider name field; see the operation reference for its format. */
    "name": string | null;
    /** Native provider image field; see the operation reference for its format. */
    "image"?: string | null;
    /** Native provider location field; see the operation reference for its format. */
    "location"?: string | null;
    /** Native provider followers field; see the operation reference for its format. */
    "followers"?: number | null;
    /** Native provider connections field; see the operation reference for its format. */
    "connections"?: string | null;
    /** Native provider about field; see the operation reference for its format. */
    "about"?: string | null;
    /** Native provider recentPosts field; see the operation reference for its format. */
    "recentPosts"?: ({
        "title"?: string | null;
        "activityType"?: string | null;
        "link"?: string | null;
        "image"?: string | null;
    })[] | null;
    /** Native provider experience field; see the operation reference for its format. */
    "experience"?: ({
        "@type"?: string | null;
        "name"?: string | null;
        "url"?: string | null;
        "location"?: string | null;
        "member"?: {
            "@type"?: string | null;
            "description"?: string | null;
        } | null;
    })[] | null;
    /** Native provider articles field; see the operation reference for its format. */
    "articles"?: ({
        "headline"?: string | null;
        "author"?: string | null;
        "datePublished"?: string | null;
        "image"?: string | null;
        "articleBody"?: string | null;
    })[] | null;
    /** Native provider activity field; see the operation reference for its format. */
    "activity"?: ({
        "title"?: string | null;
        "activityType"?: string | null;
        "link"?: string | null;
        "image"?: string | null;
    })[];
    /** Native provider education field; see the operation reference for its format. */
    "education"?: ({
        "@type"?: string | null;
        "name"?: string | null;
        "url"?: string | null;
        "member"?: {
            "@type"?: string | null;
            "startDate"?: number | null;
            "endDate"?: number | null;
        } | null;
    })[] | null;
    /** Native provider publications field; see the operation reference for its format. */
    "publications"?: ({
        "name"?: string | null;
        "url"?: string | null;
    })[] | null;
    /** Native provider projects field; see the operation reference for its format. */
    "projects"?: ({
        "name"?: string | null;
        "url"?: string | null;
        "dateRange"?: string | null;
        "description"?: string | null;
        "contributors"?: ({
            "name"?: string | null;
            "link"?: string | null;
            "image"?: string | null;
        })[] | null;
    })[] | null;
    /** Native provider recommendations field; see the operation reference for its format. */
    "recommendations"?: ({
        "name"?: string | null;
        "link"?: string | null;
        "image"?: string | null;
        "text"?: string | null;
    })[] | null;
    /** Native provider similarProfiles field; see the operation reference for its format. */
    "similarProfiles"?: ({
        "link"?: string | null;
        "name"?: string | null;
        "image"?: string | null;
    })[] | null;
}
/** Options for linkedin.getCompany. */
export interface LinkedInGetCompanyOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native linkedin getCompany fields; optional metadata may be missing or null. */
export interface LinkedInGetCompanyData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider id field; see the operation reference for its format. */
    "id"?: string | null;
    /** Native provider name field; see the operation reference for its format. */
    "name": string | null;
    /** Native provider description field; see the operation reference for its format. */
    "description"?: string | null;
    /** Native provider location field; see the operation reference for its format. */
    "location"?: {
        "city"?: string | null;
        "state"?: string | null;
        "country"?: string | null;
    } | null;
    /** Native provider employeeCount field; see the operation reference for its format. */
    "employeeCount"?: number | null;
    /** Native provider website field; see the operation reference for its format. */
    "website"?: string | null;
    /** Native provider logo field; see the operation reference for its format. */
    "logo"?: string | null;
    /** Native provider coverImage field; see the operation reference for its format. */
    "coverImage"?: string | null;
    /** Native provider slogan field; see the operation reference for its format. */
    "slogan"?: string | null;
    /** Native provider similarPages field; see the operation reference for its format. */
    "similarPages"?: ({
        "link"?: string | null;
        "name"?: string | null;
        "image"?: string | null;
    })[] | null;
    /** Native provider industry field; see the operation reference for its format. */
    "industry"?: string | null;
    /** Native provider size field; see the operation reference for its format. */
    "size"?: string | null;
    /** Native provider founded field; see the operation reference for its format. */
    "founded"?: number | null;
    /** Native provider headquarters field; see the operation reference for its format. */
    "headquarters"?: string | null;
    /** Native provider type field; see the operation reference for its format. */
    "type"?: string | null;
    /** Native provider specialties field; see the operation reference for its format. */
    "specialties"?: (string)[] | null;
    /** Native provider funding field; see the operation reference for its format. */
    "funding"?: {
        "numberOfRounds"?: number | null;
        "lastRound"?: {
            "type"?: string | null;
            "date"?: string | null;
            "amount"?: string | null;
        } | null;
        "investors"?: ({
            "name"?: string | null;
            "crunchbaseUrl"?: string | null;
            "image"?: string | null;
        })[] | null;
    } | null;
    /** Native provider employees field; see the operation reference for its format. */
    "employees"?: ({
        "name"?: string | null;
        "title"?: string | null;
        "link"?: string | null;
        "image"?: string | null;
    })[] | null;
    /** Native provider posts field; see the operation reference for its format. */
    "posts"?: ({
        "url"?: string | null;
        "datePublished"?: string | null;
        "text"?: string | null;
    })[];
}
/** Options for linkedin.listCompanyPosts. */
export interface LinkedInListCompanyPostsOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option page. See the operation reference for values and defaults. */
    page?: number;
}
/** Native linkedin listCompanyPosts fields; optional metadata may be missing or null. */
export interface LinkedInListCompanyPostsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider posts field; see the operation reference for its format. */
    "posts": ({
        "url"?: string | null;
        "id"?: string | null;
        "datePublished"?: string | null;
        "text"?: string | null;
    })[];
}
/** Options for linkedin.searchPosts. */
export interface LinkedInSearchPostsOptions {
    /** Provider option query. See the operation reference for values and defaults. */
    query: string;
    /** Provider option datePosted. See the operation reference for values and defaults. */
    datePosted?: "last-hour" | "last-day" | "last-week" | "last-month" | "last-year";
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: string;
}
/** Native linkedin searchPosts fields; optional metadata may be missing or null. */
export interface LinkedInSearchPostsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider query field; see the operation reference for its format. */
    "query"?: string | null;
    /** Native provider posts field; see the operation reference for its format. */
    "posts": ({
        "url"?: string | null;
        "datePublished"?: string | null;
        "description"?: string | null;
        "media"?: string | null;
        "images"?: (string)[] | null;
        "image"?: string | null;
        "author"?: {
            "name"?: string | null;
            "url"?: string | null;
            "image"?: string | null;
            "followers"?: number | null;
        } | null;
        "comments"?: ({
            "author"?: string | null;
            "text"?: string | null;
            "linkedinUrl"?: string | null;
        })[] | null;
        "likeCount"?: number | null;
        "commentCount"?: number | null;
    })[];
    /** Native next-page token; preserve its type and stop when null or absent. */
    "cursor"?: string | null;
}
/** Options for linkedin.getPost. */
export interface LinkedInGetPostOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native linkedin getPost fields; optional metadata may be missing or null. */
export interface LinkedInGetPostData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider activityUrn field; see the operation reference for its format. */
    "activityUrn"?: unknown;
    /** Native provider contentUrn field; see the operation reference for its format. */
    "contentUrn"?: unknown;
    /** Native provider url field; see the operation reference for its format. */
    "url": string | null;
    /** Native provider name field; see the operation reference for its format. */
    "name"?: string | null;
    /** Native provider headline field; see the operation reference for its format. */
    "headline"?: string | null;
    /** Native provider commentCount field; see the operation reference for its format. */
    "commentCount"?: number | null;
    /** Native provider likeCount field; see the operation reference for its format. */
    "likeCount"?: number | null;
    /** Native provider datePublished field; see the operation reference for its format. */
    "datePublished"?: string | null;
    /** Native provider description field; see the operation reference for its format. */
    "description"?: string | null;
    /** Native provider author field; see the operation reference for its format. */
    "author"?: {
        "name"?: string | null;
        "url"?: string | null;
        "followers"?: number | null;
    } | null;
    /** Native provider comments field; see the operation reference for its format. */
    "comments"?: ({
        "author"?: string | null;
        "text"?: string | null;
        "linkedinUrl"?: string | null;
    })[];
    /** Native provider moreArticles field; see the operation reference for its format. */
    "moreArticles"?: ({
        "link"?: string | null;
        "title"?: string | null;
        "datePublished"?: string | null;
        "description"?: string | null;
        "reactionCount"?: number | null;
        "commentCount"?: number | null;
    })[] | null;
}
/** Options for linkedin.getTranscript. */
export interface LinkedInGetTranscriptOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native linkedin getTranscript fields; optional metadata may be missing or null. */
export interface LinkedInGetTranscriptData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider url field; see the operation reference for its format. */
    "url"?: string | null;
    /** Native transcript text or timestamped segments; null indicates unavailable captions. */
    "transcript": string | null;
    /** Native provider transcriptNotAvailable field; see the operation reference for its format. */
    "transcriptNotAvailable"?: boolean | null;
}
/** Options for github.getUser. */
export interface GitHubGetUserOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option url. See the operation reference for values and defaults. */
    url?: string;
}
/** Native github getUser fields; optional metadata may be missing or null. */
export interface GitHubGetUserData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider id field; see the operation reference for its format. */
    "id"?: number | null;
    /** Native provider node_id field; see the operation reference for its format. */
    "node_id"?: string | null;
    /** Native provider login field; see the operation reference for its format. */
    "login": string | null;
    /** Native provider name field; see the operation reference for its format. */
    "name"?: string | null;
    /** Native provider company field; see the operation reference for its format. */
    "company"?: string | null;
    /** Native provider blog field; see the operation reference for its format. */
    "blog"?: string | null;
    /** Native provider location field; see the operation reference for its format. */
    "location"?: string | null;
    /** Native provider email field; see the operation reference for its format. */
    "email"?: string | null;
    /** Native provider bio field; see the operation reference for its format. */
    "bio"?: unknown;
    /** Native provider twitter_username field; see the operation reference for its format. */
    "twitter_username"?: unknown;
    /** Native provider avatar_url field; see the operation reference for its format. */
    "avatar_url"?: string | null;
    /** Native provider url field; see the operation reference for its format. */
    "url"?: string | null;
    /** Native provider type field; see the operation reference for its format. */
    "type"?: string | null;
    /** Native provider site_admin field; see the operation reference for its format. */
    "site_admin"?: boolean | null;
    /** Native provider public_repos field; see the operation reference for its format. */
    "public_repos"?: number | null;
    /** Native provider public_gists field; see the operation reference for its format. */
    "public_gists"?: number | null;
    /** Native provider followers field; see the operation reference for its format. */
    "followers"?: number | null;
    /** Native provider following field; see the operation reference for its format. */
    "following"?: number | null;
    /** Native provider created_at field; see the operation reference for its format. */
    "created_at"?: string | null;
    /** Native provider updated_at field; see the operation reference for its format. */
    "updated_at"?: string | null;
}
/** Options for github.listUserRepositories. */
export interface GitHubListUserRepositoriesOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option url. See the operation reference for values and defaults. */
    url?: string;
    /** Provider option type. See the operation reference for values and defaults. */
    type?: "owner" | "all" | "member";
    /** Provider option sort. See the operation reference for values and defaults. */
    sort?: "created" | "updated" | "pushed" | "full_name";
    /** Provider option direction. See the operation reference for values and defaults. */
    direction?: "asc" | "desc";
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: number;
}
/** Native github listUserRepositories fields; optional metadata may be missing or null. */
export interface GitHubListUserRepositoriesData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider repositories field; see the operation reference for its format. */
    "repositories": ({
        "id"?: number | null;
        "node_id"?: string | null;
        "name"?: string | null;
        "full_name"?: string | null;
        "owner"?: {
            "id"?: number | null;
            "login"?: string | null;
            "avatar_url"?: string | null;
            "url"?: string | null;
            "type"?: string | null;
        } | null;
        "private"?: boolean | null;
        "url"?: string | null;
        "description"?: string | null;
        "fork"?: boolean | null;
        "homepage"?: string | null;
        "language"?: string | null;
        "forks_count"?: number | null;
        "stars_count"?: number | null;
        "watchers_count"?: number | null;
        "size"?: number | null;
        "default_branch"?: string | null;
        "open_issues_count"?: number | null;
        "topics"?: (string)[] | null;
        "license"?: {
            "key"?: string | null;
            "name"?: string | null;
            "spdx_id"?: string | null;
        } | null;
        "archived"?: boolean | null;
        "disabled"?: boolean | null;
        "visibility"?: string | null;
        "pushed_at"?: string | null;
        "created_at"?: string | null;
        "updated_at"?: string | null;
    })[];
    /** Native next-page token; preserve its type and stop when null or absent. */
    "cursor"?: number | null;
}
/** Options for github.listUserPullRequests. */
export interface GitHubListUserPullRequestsOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle: string;
    /** Provider option since. See the operation reference for values and defaults. */
    since?: string;
    /** Provider option until. See the operation reference for values and defaults. */
    until?: string;
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: number;
}
/** Native github listUserPullRequests fields; optional metadata may be missing or null. */
export interface GitHubListUserPullRequestsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider pull_requests field; see the operation reference for its format. */
    "pull_requests": ({
        "title"?: string | null;
        "repo"?: string | null;
        "state"?: string | null;
        "created_at"?: string | null;
        "url"?: string | null;
    })[];
    /** Native next-page token; preserve its type and stop when null or absent. */
    "cursor"?: number | null;
    /** Native provider has_more field; see the operation reference for its format. */
    "has_more"?: boolean | null;
}
/** Options for github.listUserActivity. */
export interface GitHubListUserActivityOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option url. See the operation reference for values and defaults. */
    url?: string;
    /** Provider option year. See the operation reference for values and defaults. */
    year?: number;
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: number;
}
/** Native github listUserActivity fields; optional metadata may be missing or null. */
export interface GitHubListUserActivityData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider username field; see the operation reference for its format. */
    "username"?: string | null;
    /** Native provider year field; see the operation reference for its format. */
    "year"?: number | null;
    /** Native provider month field; see the operation reference for its format. */
    "month"?: string | null;
    /** Native provider activity field; see the operation reference for its format. */
    "activity": ({
        "summary"?: string | null;
        "details"?: ({
            "text"?: string | null;
            "repository"?: {
                "text"?: string | null;
                "url"?: string | null;
            } | null;
            "links"?: ({
                "text"?: string | null;
                "url"?: string | null;
            })[] | null;
            "language"?: string | null;
            "date_text"?: unknown;
            "date_description"?: unknown;
        })[] | null;
    })[];
    /** Native provider no_activity field; see the operation reference for its format. */
    "no_activity"?: boolean | null;
    /** Native provider message field; see the operation reference for its format. */
    "message"?: string | null;
    /** Native next-page token; preserve its type and stop when null or absent. */
    "cursor"?: number | null;
}
/** Options for github.listFollowers. */
export interface GitHubListFollowersOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option url. See the operation reference for values and defaults. */
    url?: string;
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: number;
}
/** Native github listFollowers fields; optional metadata may be missing or null. */
export interface GitHubListFollowersData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider followers field; see the operation reference for its format. */
    "followers": ({
        "id"?: number | null;
        "node_id"?: string | null;
        "login"?: string | null;
        "avatar_url"?: string | null;
        "url"?: string | null;
        "type"?: string | null;
        "site_admin"?: boolean | null;
    })[];
    /** Native next-page token; preserve its type and stop when null or absent. */
    "cursor"?: number | null;
}
/** Options for github.listFollowing. */
export interface GitHubListFollowingOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option url. See the operation reference for values and defaults. */
    url?: string;
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: number;
}
/** Native github listFollowing fields; optional metadata may be missing or null. */
export interface GitHubListFollowingData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider following field; see the operation reference for its format. */
    "following": ({
        "id"?: number | null;
        "node_id"?: string | null;
        "login"?: string | null;
        "avatar_url"?: string | null;
        "url"?: string | null;
        "type"?: string | null;
        "site_admin"?: boolean | null;
    })[];
    /** Native next-page token; preserve its type and stop when null or absent. */
    "cursor"?: number | null;
}
/** Options for github.getContributions. */
export interface GitHubGetContributionsOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle?: string;
    /** Provider option url. See the operation reference for values and defaults. */
    url?: string;
    /** Provider option year. See the operation reference for values and defaults. */
    year?: number;
}
/** Native github getContributions fields; optional metadata may be missing or null. */
export interface GitHubGetContributionsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider contributions field; see the operation reference for its format. */
    "contributions": {
        "username"?: string | null;
        "year"?: number | null;
        "total_contributions"?: number | null;
        "days"?: ({
            "date"?: string | null;
            "contribution_count"?: number | null;
            "color"?: unknown;
            "intensity"?: number | null;
        })[] | null;
    } | null;
}
/** Options for github.getRepository. */
export interface GitHubGetRepositoryOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native github getRepository fields; optional metadata may be missing or null. */
export interface GitHubGetRepositoryData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider repository field; see the operation reference for its format. */
    "repository": {
        "id"?: number | null;
        "node_id"?: string | null;
        "name"?: string | null;
        "full_name"?: string | null;
        "owner"?: {
            "id"?: number | null;
            "login"?: string | null;
            "avatar_url"?: string | null;
            "url"?: string | null;
            "type"?: string | null;
        } | null;
        "private"?: boolean | null;
        "url"?: string | null;
        "description"?: string | null;
        "fork"?: boolean | null;
        "homepage"?: string | null;
        "language"?: string | null;
        "forks_count"?: number | null;
        "stars_count"?: number | null;
        "watchers_count"?: number | null;
        "size"?: number | null;
        "default_branch"?: string | null;
        "open_issues_count"?: number | null;
        "topics"?: unknown[] | null;
        "license"?: {
            "key"?: string | null;
            "name"?: string | null;
            "spdx_id"?: string | null;
        } | null;
        "archived"?: boolean | null;
        "disabled"?: boolean | null;
        "visibility"?: string | null;
        "pushed_at"?: string | null;
        "created_at"?: string | null;
        "updated_at"?: string | null;
    } | null;
}
/** Options for github.listTrendingRepositories. */
export interface GitHubListTrendingRepositoriesOptions {
    /** Provider option language. See the operation reference for values and defaults. */
    language?: string;
    /** Provider option since. See the operation reference for values and defaults. */
    since?: "daily" | "weekly" | "monthly";
    /** Provider option spokenLanguageCode. See the operation reference for values and defaults. */
    spokenLanguageCode?: string;
}
/** Native github listTrendingRepositories fields; optional metadata may be missing or null. */
export interface GitHubListTrendingRepositoriesData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider since field; see the operation reference for its format. */
    "since"?: string | null;
    /** Native provider language field; see the operation reference for its format. */
    "language"?: string | null;
    /** Native provider spoken_language_code field; see the operation reference for its format. */
    "spoken_language_code"?: unknown;
    /** Native provider repositories field; see the operation reference for its format. */
    "repositories": ({
        "rank"?: number | null;
        "owner"?: string | null;
        "repo"?: string | null;
        "full_name"?: string | null;
        "url"?: string | null;
        "description"?: string | null;
        "language"?: string | null;
        "language_color"?: string | null;
        "stars_count"?: number | null;
        "forks_count"?: number | null;
        "stars_today"?: number | null;
        "stars_today_text"?: string | null;
        "built_by"?: ({
            "username"?: string | null;
            "avatar_url"?: string | null;
        })[] | null;
    })[];
}
/** Options for github.listTrendingDevelopers. */
export interface GitHubListTrendingDevelopersOptions {
    /** Provider option language. See the operation reference for values and defaults. */
    language?: string;
    /** Provider option since. See the operation reference for values and defaults. */
    since?: "daily" | "weekly" | "monthly";
}
/** Native github listTrendingDevelopers fields; optional metadata may be missing or null. */
export interface GitHubListTrendingDevelopersData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider since field; see the operation reference for its format. */
    "since"?: string | null;
    /** Native provider language field; see the operation reference for its format. */
    "language"?: string | null;
    /** Native provider developers field; see the operation reference for its format. */
    "developers": ({
        "rank"?: number | null;
        "username"?: string | null;
        "name"?: string | null;
        "url"?: string | null;
        "avatar_url"?: string | null;
        "popular_repository"?: {
            "owner"?: string | null;
            "repo"?: string | null;
            "full_name"?: string | null;
            "url"?: string | null;
            "name"?: string | null;
            "description"?: string | null;
        } | null;
    })[];
}
/** Options for twitter.searchPosts. */
export interface TwitterSearchPostsOptions {
    /** Provider option query. See the operation reference for values and defaults. */
    query: string;
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: string;
}
/** Native twitter searchPosts fields; optional metadata may be missing or null. */
export interface TwitterSearchPostsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider query field; see the operation reference for its format. */
    "query"?: string | null;
    /** Native provider posts field; see the operation reference for its format. */
    "posts": ({
        "rest_id"?: string | null;
        "url"?: string | null;
        "views"?: {
            "count"?: string | null;
            "state"?: string | null;
        } | null;
        "legacy"?: {
            "created_at"?: string | null;
            "full_text"?: string | null;
            "favorite_count"?: number | null;
            "retweet_count"?: number | null;
            "reply_count"?: number | null;
            "quote_count"?: number | null;
            "bookmark_count"?: number | null;
        } | null;
    })[];
    /** Native next-page token; preserve its type and stop when null or absent. */
    "cursor"?: string | null;
}
/** Options for twitter.getProfile. */
export interface TwitterGetProfileOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle: string;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native twitter getProfile fields; optional metadata may be missing or null. */
export interface TwitterGetProfileData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider __typename field; see the operation reference for its format. */
    "__typename"?: string | null;
    /** Native provider id field; see the operation reference for its format. */
    "id"?: string | null;
    /** Native provider rest_id field; see the operation reference for its format. */
    "rest_id": string | null;
    /** Native provider affiliates_highlighted_label field; see the operation reference for its format. */
    "affiliates_highlighted_label"?: {
        "label"?: {
            "url"?: {
                "url"?: string | null;
                "urlType"?: string | null;
            } | null;
            "badge"?: {
                "url"?: string | null;
            } | null;
            "description"?: string | null;
            "userLabelType"?: string | null;
            "userLabelDisplayType"?: string | null;
        } | null;
    } | null;
    /** Native provider is_blue_verified field; see the operation reference for its format. */
    "is_blue_verified"?: boolean | null;
    /** Native provider profile_image_shape field; see the operation reference for its format. */
    "profile_image_shape"?: string | null;
    /** Native provider legacy field; see the operation reference for its format. */
    "legacy"?: {
        "created_at"?: string | null;
        "default_profile"?: boolean | null;
        "default_profile_image"?: boolean | null;
        "description"?: string | null;
        "entities"?: {
            "description"?: {
                "urls"?: (unknown)[] | null;
            } | null;
            "url"?: {
                "urls"?: (unknown)[] | null;
            } | null;
        } | null;
        "fast_followers_count"?: number | null;
        "favourites_count"?: number | null;
        "followers_count"?: number | null;
        "friends_count"?: number | null;
        "has_custom_timelines"?: boolean | null;
        "is_translator"?: boolean | null;
        "listed_count"?: number | null;
        "location"?: string | null;
        "media_count"?: number | null;
        "name"?: string | null;
        "normal_followers_count"?: number | null;
        "pinned_tweet_ids_str"?: (string)[] | null;
        "possibly_sensitive"?: boolean | null;
        "profile_banner_url"?: string | null;
        "profile_image_url_https"?: string | null;
        "profile_interstitial_type"?: string | null;
        "screen_name"?: string | null;
        "statuses_count"?: number | null;
        "translator_type"?: string | null;
        "url"?: string | null;
        "verified"?: boolean | null;
        "withheld_in_countries"?: unknown[] | null;
    } | null;
    /** Native provider tipjar_settings field; see the operation reference for its format. */
    "tipjar_settings"?: {
        "is_enabled"?: boolean | null;
        "bandcamp_handle"?: string | null;
        "bitcoin_handle"?: string | null;
        "cash_app_handle"?: string | null;
        "ethereum_handle"?: string | null;
        "gofundme_handle"?: string | null;
        "patreon_handle"?: string | null;
        "pay_pal_handle"?: string | null;
        "venmo_handle"?: string | null;
    } | null;
    /** Native provider legacy_extended_profile field; see the operation reference for its format. */
    "legacy_extended_profile"?: unknown;
    /** Native provider is_profile_translatable field; see the operation reference for its format. */
    "is_profile_translatable"?: boolean | null;
    /** Native provider has_hidden_subscriptions_on_profile field; see the operation reference for its format. */
    "has_hidden_subscriptions_on_profile"?: boolean | null;
    /** Native provider verification_info field; see the operation reference for its format. */
    "verification_info"?: {
        "is_identity_verified"?: boolean | null;
        "reason"?: {
            "description"?: {
                "text"?: string | null;
                "entities"?: (unknown)[] | null;
            } | null;
            "verified_since_msec"?: string | null;
        } | null;
    } | null;
    /** Native provider highlights_info field; see the operation reference for its format. */
    "highlights_info"?: {
        "can_highlight_tweets"?: boolean | null;
        "highlighted_tweets"?: string | null;
    } | null;
    /** Native provider user_seed_tweet_count field; see the operation reference for its format. */
    "user_seed_tweet_count"?: number | null;
    /** Native provider business_account field; see the operation reference for its format. */
    "business_account"?: unknown;
    /** Native provider creator_subscriptions_count field; see the operation reference for its format. */
    "creator_subscriptions_count"?: number | null;
}
/** Options for twitter.listUserTweets. */
export interface TwitterListUserTweetsOptions {
    /** Provider option handle. See the operation reference for values and defaults. */
    handle: string;
    /** Provider option trim. See the operation reference for values and defaults. */
    trim?: boolean;
}
/** Native twitter listUserTweets fields; optional metadata may be missing or null. */
export interface TwitterListUserTweetsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider tweets field; see the operation reference for its format. */
    "tweets": ({
        "rest_id"?: string | null;
        "views"?: {
            "count"?: string | null;
            "state"?: string | null;
        } | null;
        "source"?: string | null;
        "legacy"?: {
            "bookmark_count"?: number | null;
            "bookmarked"?: boolean | null;
            "created_at"?: string | null;
            "conversation_id_str"?: string | null;
            "display_text_range"?: (number)[] | null;
            "entities"?: {
                "hashtags"?: unknown[] | null;
                "symbols"?: unknown[] | null;
                "timestamps"?: unknown[] | null;
                "urls"?: unknown[] | null;
                "user_mentions"?: unknown[] | null;
            } | null;
            "favorite_count"?: number | null;
            "favorited"?: boolean | null;
            "full_text"?: string | null;
            "is_quote_status"?: boolean | null;
            "lang"?: string | null;
            "quote_count"?: number | null;
            "reply_count"?: number | null;
            "retweet_count"?: number | null;
            "retweeted"?: boolean | null;
            "user_id_str"?: string | null;
            "id_str"?: string | null;
        } | null;
        "url"?: string | null;
    })[];
}
/** Options for twitter.getTweet. */
export interface TwitterGetTweetOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option trim. See the operation reference for values and defaults. */
    trim?: boolean;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native twitter getTweet fields; optional metadata may be missing or null. */
export interface TwitterGetTweetData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider rest_id field; see the operation reference for its format. */
    "rest_id": string | null;
    /** Native provider views field; see the operation reference for its format. */
    "views"?: {
        "count"?: string | null;
        "state"?: string | null;
    } | null;
    /** Native provider source field; see the operation reference for its format. */
    "source"?: string | null;
    /** Native provider legacy field; see the operation reference for its format. */
    "legacy"?: {
        "bookmark_count"?: number | null;
        "bookmarked"?: boolean | null;
        "created_at"?: string | null;
        "conversation_id_str"?: string | null;
        "display_text_range"?: (number)[] | null;
        "entities"?: {
            "hashtags"?: unknown[] | null;
            "media"?: ({
                "display_url"?: string | null;
                "expanded_url"?: string | null;
                "id_str"?: string | null;
                "indices"?: (number)[] | null;
                "media_key"?: string | null;
                "media_url_https"?: string | null;
                "type"?: string | null;
                "url"?: string | null;
                "ext_media_availability"?: unknown;
                "allow_download_status"?: unknown;
                "media_results"?: unknown;
            })[] | null;
            "symbols"?: unknown[] | null;
            "timestamps"?: unknown[] | null;
            "urls"?: unknown[] | null;
            "user_mentions"?: unknown[] | null;
        } | null;
        "extended_entities"?: {
            "media"?: ({
                "display_url"?: string | null;
                "expanded_url"?: string | null;
                "id_str"?: string | null;
                "indices"?: (number)[] | null;
                "media_key"?: string | null;
                "media_url_https"?: string | null;
                "type"?: string | null;
                "url"?: string | null;
                "ext_media_availability"?: unknown;
                "allow_download_status"?: unknown;
                "media_results"?: unknown;
            })[] | null;
        } | null;
        "favorite_count"?: number | null;
        "favorited"?: boolean | null;
        "full_text"?: string | null;
        "is_quote_status"?: boolean | null;
        "lang"?: string | null;
        "possibly_sensitive"?: boolean | null;
        "possibly_sensitive_editable"?: boolean | null;
        "quote_count"?: number | null;
        "reply_count"?: number | null;
        "retweet_count"?: number | null;
        "retweeted"?: boolean | null;
        "user_id_str"?: string | null;
        "id_str"?: string | null;
    } | null;
}
/** Options for twitter.getTranscript. */
export interface TwitterGetTranscriptOptions {
    /** Provider option allowLongTranscription. See the operation reference for values and defaults. */
    allowLongTranscription?: boolean;
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native twitter getTranscript fields; optional metadata may be missing or null. */
export interface TwitterGetTranscriptData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native transcript text or timestamped segments; null indicates unavailable captions. */
    "transcript": string | null;
}
/** Options for twitter.getCommunity. */
export interface TwitterGetCommunityOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native twitter getCommunity fields; optional metadata may be missing or null. */
export interface TwitterGetCommunityData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider __typename field; see the operation reference for its format. */
    "__typename"?: string | null;
    /** Native provider is_member field; see the operation reference for its format. */
    "is_member"?: boolean | null;
    /** Native provider name field; see the operation reference for its format. */
    "name"?: string | null;
    /** Native provider role field; see the operation reference for its format. */
    "role"?: string | null;
    /** Native provider rest_id field; see the operation reference for its format. */
    "rest_id": string | null;
    /** Native provider actions field; see the operation reference for its format. */
    "actions"?: {
        "join_action_result"?: {
            "__typename"?: string | null;
        } | null;
        "id"?: string | null;
    } | null;
    /** Native provider description field; see the operation reference for its format. */
    "description"?: string | null;
    /** Native provider creator_results field; see the operation reference for its format. */
    "creator_results"?: {
        "result"?: {
            "__typename"?: string | null;
            "id"?: string | null;
            "is_blue_verified"?: boolean | null;
            "core"?: {
                "screen_name"?: string | null;
            } | null;
            "verification"?: {
                "verified"?: boolean | null;
            } | null;
        } | null;
        "id"?: string | null;
    } | null;
    /** Native provider join_policy field; see the operation reference for its format. */
    "join_policy"?: string | null;
    /** Native provider created_at field; see the operation reference for its format. */
    "created_at"?: number | null;
    /** Native provider rules field; see the operation reference for its format. */
    "rules"?: ({
        "rest_id"?: string | null;
        "description"?: string | null;
        "name"?: string | null;
        "id"?: string | null;
    })[] | null;
    /** Native provider members_facepile_results field; see the operation reference for its format. */
    "members_facepile_results"?: ({
        "result"?: {
            "__typename"?: string | null;
            "avatar"?: {
                "image_url"?: string | null;
            } | null;
            "id"?: string | null;
        } | null;
        "id"?: string | null;
    })[] | null;
    /** Native provider member_count field; see the operation reference for its format. */
    "member_count"?: number | null;
    /** Native provider id field; see the operation reference for its format. */
    "id"?: string | null;
}
/** Options for twitter.listCommunityTweets. */
export interface TwitterListCommunityTweetsOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native twitter listCommunityTweets fields; optional metadata may be missing or null. */
export interface TwitterListCommunityTweetsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider tweets field; see the operation reference for its format. */
    "tweets": ({
        "id"?: string | null;
        "source"?: string | null;
        "view_count"?: string | null;
        "bookmark_count"?: number | null;
        "bookmarked"?: boolean | null;
        "created_at"?: string | null;
        "conversation_id_str"?: string | null;
        "display_text_range"?: (number)[] | null;
        "favorite_count"?: number | null;
        "favorited"?: boolean | null;
        "full_text"?: string | null;
        "is_quote_status"?: boolean | null;
        "lang"?: string | null;
        "possibly_sensitive"?: boolean | null;
        "possibly_sensitive_editable"?: boolean | null;
        "quote_count"?: number | null;
        "reply_count"?: number | null;
        "retweet_count"?: number | null;
        "retweeted"?: boolean | null;
        "user_id_str"?: string | null;
        "id_str"?: string | null;
        "user"?: {
            "__typename"?: string | null;
            "id"?: string | null;
            "rest_id"?: string | null;
            "affiliates_highlighted_label"?: unknown;
            "avatar"?: {
                "image_url"?: string | null;
            } | null;
            "core"?: {
                "created_at"?: string | null;
                "name"?: string | null;
                "screen_name"?: string | null;
            } | null;
            "dm_permissions"?: unknown;
            "is_blue_verified"?: boolean | null;
            "legacy"?: {
                "default_profile"?: boolean | null;
                "default_profile_image"?: boolean | null;
                "description"?: string | null;
                "entities"?: unknown;
                "fast_followers_count"?: number | null;
                "favourites_count"?: number | null;
                "followers_count"?: number | null;
                "friends_count"?: number | null;
                "has_custom_timelines"?: boolean | null;
                "is_translator"?: boolean | null;
                "listed_count"?: number | null;
                "media_count"?: number | null;
                "normal_followers_count"?: number | null;
                "pinned_tweet_ids_str"?: (string)[] | null;
                "possibly_sensitive"?: boolean | null;
                "profile_interstitial_type"?: string | null;
                "statuses_count"?: number | null;
                "translator_type"?: string | null;
                "withheld_in_countries"?: unknown[] | null;
            } | null;
            "location"?: {
                "location"?: string | null;
            } | null;
            "media_permissions"?: unknown;
            "parody_commentary_fan_label"?: string | null;
            "profile_image_shape"?: string | null;
            "professional"?: {
                "rest_id"?: string | null;
                "professional_type"?: string | null;
                "category"?: (unknown)[] | null;
            } | null;
            "privacy"?: {
                "protected"?: boolean | null;
            } | null;
            "relationship_perspectives"?: unknown;
            "tipjar_settings"?: {
                "is_enabled"?: boolean | null;
                "bandcamp_handle"?: string | null;
                "bitcoin_handle"?: string | null;
                "cash_app_handle"?: string | null;
                "ethereum_handle"?: string | null;
                "gofundme_handle"?: string | null;
                "patreon_handle"?: string | null;
                "pay_pal_handle"?: string | null;
                "venmo_handle"?: string | null;
            } | null;
            "verification"?: {
                "verified"?: boolean | null;
            } | null;
        } | null;
    })[];
}
/** Options for reddit.getSubreddit. */
export interface RedditGetSubredditOptions {
    /** Provider option subreddit. See the operation reference for values and defaults. */
    subreddit?: string;
    /** Provider option url. See the operation reference for values and defaults. */
    url?: string;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native reddit getSubreddit fields; optional metadata may be missing or null. */
export interface RedditGetSubredditData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider subreddit_id field; see the operation reference for its format. */
    "subreddit_id"?: string | null;
    /** Native provider display_name field; see the operation reference for its format. */
    "display_name": string | null;
    /** Native provider weekly_active_users field; see the operation reference for its format. */
    "weekly_active_users"?: number | null;
    /** Native provider weekly_contributions field; see the operation reference for its format. */
    "weekly_contributions"?: number | null;
    /** Native provider rules field; see the operation reference for its format. */
    "rules"?: string | null;
    /** Native provider description field; see the operation reference for its format. */
    "description"?: string | null;
    /** Native provider header_img field; see the operation reference for its format. */
    "header_img"?: string | null;
    /** Native provider icon_img field; see the operation reference for its format. */
    "icon_img"?: string | null;
    /** Native provider subscribers field; see the operation reference for its format. */
    "subscribers"?: number | null;
    /** Native provider advertiser_category field; see the operation reference for its format. */
    "advertiser_category"?: string | null;
    /** Native provider created_at field; see the operation reference for its format. */
    "created_at"?: string | null;
    /** Native provider submit_text field; see the operation reference for its format. */
    "submit_text"?: string | null;
}
/** Options for reddit.listSubredditPosts. */
export interface RedditListSubredditPostsOptions {
    /** Provider option subreddit. See the operation reference for values and defaults. */
    subreddit: string;
    /** Provider option timeframe. See the operation reference for values and defaults. */
    timeframe?: "all" | "day" | "week" | "month" | "year";
    /** Provider option sort. See the operation reference for values and defaults. */
    sort?: "best" | "hot" | "new" | "top" | "rising";
    /** Provider option after. See the operation reference for values and defaults. */
    after?: string;
    /** Provider option trim. See the operation reference for values and defaults. */
    trim?: boolean;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native reddit listSubredditPosts fields; optional metadata may be missing or null. */
export interface RedditListSubredditPostsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider posts field; see the operation reference for its format. */
    "posts": ({
        "id"?: string | null;
        "author"?: string | null;
        "author_fullname"?: string | null;
        "subreddit"?: string | null;
        "title"?: string | null;
        "link_flair_text"?: unknown;
        "selftext"?: string | null;
        "selftext_html"?: string | null;
        "downs"?: number | null;
        "name"?: string | null;
        "upvote_ratio"?: number | null;
        "ups"?: number | null;
        "total_awards_received"?: number | null;
        "score"?: number | null;
        "created"?: number | null;
        "num_comments"?: number | null;
        "url"?: string | null;
        "subreddit_subscribers"?: number | null;
        "is_video"?: boolean | null;
        "created_utc"?: number | null;
    })[];
    /** Opaque Reddit next-page token; pass back unchanged. */
    "after"?: string | null;
}
/** Options for reddit.searchSubreddit. */
export interface RedditSearchSubredditOptions {
    /** Provider option subreddit. See the operation reference for values and defaults. */
    subreddit: string;
    /** Provider option query. See the operation reference for values and defaults. */
    query?: string;
    /** Provider option sort. See the operation reference for values and defaults. */
    sort?: "relevance" | "hot" | "top" | "new" | "comments";
    /** Provider option timeframe. See the operation reference for values and defaults. */
    timeframe?: "all" | "year" | "month" | "week" | "day" | "hour";
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: string;
}
/** Native reddit searchSubreddit fields; optional metadata may be missing or null. */
export interface RedditSearchSubredditData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider posts field; see the operation reference for its format. */
    "posts": ({
        "id"?: string | null;
        "post_id"?: string | null;
        "title"?: string | null;
        "url"?: string | null;
        "permalink"?: string | null;
        "nsfw"?: boolean | null;
        "spoiler"?: boolean | null;
        "is_crosspost"?: boolean | null;
        "subreddit"?: string | {
            "id"?: string | null;
            "name"?: string | null;
            "nsfw"?: boolean | null;
            "quarantined"?: boolean | null;
            "icon"?: string | null;
            "banner"?: string | null;
            "description"?: string | null;
            "weekly_visitors"?: number | null;
            "weekly_contributions"?: number | null;
        } | null;
        "author"?: string | null;
        "author_fullname"?: string | null;
        "selftext"?: string | null;
        "selftext_html"?: string | null;
        "downs"?: number | null;
        "name"?: string | null;
        "upvote_ratio"?: number | null;
        "ups"?: number | null;
        "total_awards_received"?: number | null;
        "score"?: number | null;
        "created"?: number | null;
        "created_utc"?: number | null;
        "subreddit_subscribers"?: number | null;
        "is_video"?: boolean | null;
        "over_18"?: boolean | null;
        "votes"?: number | null;
        "num_comments"?: number | null;
        "created_at"?: string | null;
        "created_at_iso"?: string | null;
        "thumbnail"?: string | null;
        "thumbnail_blurred"?: boolean | null;
        "position"?: number | null;
        "relative_position"?: number | null;
    })[];
    /** Native provider comments field; see the operation reference for its format. */
    "comments"?: ({
        "id"?: string | null;
        "post_id"?: string | null;
        "parent_comment_id"?: string | null;
        "is_reply_to_comment"?: boolean | null;
        "author"?: string | null;
        "author_avatar"?: string | null;
        "author_nsfw"?: boolean | null;
        "body"?: string | null;
        "body_html"?: string | null;
        "images"?: unknown[] | null;
        "votes"?: number | null;
        "url"?: string | null;
        "permalink"?: string | null;
        "created_at"?: string | null;
        "created_at_iso"?: string | null;
        "post"?: {
            "id"?: string | null;
            "title"?: string | null;
            "url"?: string | null;
            "permalink"?: string | null;
            "nsfw"?: boolean | null;
            "spoiler"?: boolean | null;
            "votes"?: number | null;
            "num_comments"?: number | null;
            "created_at"?: string | null;
            "created_at_iso"?: string | null;
        } | null;
        "subreddit"?: {
            "id"?: string | null;
            "name"?: string | null;
            "nsfw"?: boolean | null;
            "quarantined"?: boolean | null;
            "icon"?: string | null;
        } | null;
        "position"?: number | null;
    })[] | null;
    /** Native provider media field; see the operation reference for its format. */
    "media"?: ({
        "id"?: string | null;
        "title"?: string | null;
        "url"?: string | null;
        "permalink"?: string | null;
        "media_type"?: string | null;
        "video"?: unknown;
        "image"?: {
            "src"?: string | null;
            "width"?: number | null;
            "height"?: number | null;
            "resolutions"?: ({
                "url"?: string | null;
                "width"?: number | null;
            })[] | null;
        } | null;
        "gallery_count"?: number | null;
        "aspect_ratio"?: number | null;
        "nsfw"?: boolean | null;
        "spoiler"?: boolean | null;
        "is_blurred"?: boolean | null;
        "subreddit"?: {
            "id"?: string | null;
            "name"?: string | null;
            "nsfw"?: boolean | null;
            "quarantined"?: boolean | null;
        } | null;
        "position"?: number | null;
        "safe_search"?: string | null;
    })[] | null;
    /** Native next-page token; preserve its type and stop when null or absent. */
    "cursor"?: string | null;
}
/** Options for reddit.getPost. */
export interface RedditGetPostOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
}
/** Native reddit getPost fields; optional metadata may be missing or null. */
export interface RedditGetPostData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider id field; see the operation reference for its format. */
    "id": string | null;
    /** Native provider author field; see the operation reference for its format. */
    "author"?: string | null;
    /** Native provider author_fullname field; see the operation reference for its format. */
    "author_fullname"?: string | null;
    /** Native provider subreddit field; see the operation reference for its format. */
    "subreddit"?: string | null;
    /** Native provider title field; see the operation reference for its format. */
    "title"?: string | null;
    /** Native provider selftext field; see the operation reference for its format. */
    "selftext"?: string | null;
    /** Native provider downs field; see the operation reference for its format. */
    "downs"?: number | null;
    /** Native provider name field; see the operation reference for its format. */
    "name"?: string | null;
    /** Native provider upvote_ratio field; see the operation reference for its format. */
    "upvote_ratio"?: number | null;
    /** Native provider ups field; see the operation reference for its format. */
    "ups"?: number | null;
    /** Native provider total_awards_received field; see the operation reference for its format. */
    "total_awards_received"?: number | null;
    /** Native provider score field; see the operation reference for its format. */
    "score"?: number | null;
    /** Native provider created field; see the operation reference for its format. */
    "created"?: number | null;
    /** Native provider created_utc field; see the operation reference for its format. */
    "created_utc"?: number | null;
    /** Native provider created_at_iso field; see the operation reference for its format. */
    "created_at_iso"?: string | null;
    /** Native provider num_comments field; see the operation reference for its format. */
    "num_comments"?: number | null;
    /** Native provider url field; see the operation reference for its format. */
    "url"?: string | null;
    /** Native provider permalink field; see the operation reference for its format. */
    "permalink"?: string | null;
    /** Native provider subreddit_subscribers field; see the operation reference for its format. */
    "subreddit_subscribers"?: number | null;
    /** Native provider is_video field; see the operation reference for its format. */
    "is_video"?: boolean | null;
    /** Native provider archived field; see the operation reference for its format. */
    "archived"?: boolean | null;
    /** Native provider locked field; see the operation reference for its format. */
    "locked"?: boolean | null;
    /** Native provider over_18 field; see the operation reference for its format. */
    "over_18"?: boolean | null;
    /** Native provider spoiler field; see the operation reference for its format. */
    "spoiler"?: boolean | null;
}
/** Options for reddit.listComments. */
export interface RedditListCommentsOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option cursor. See the operation reference for values and defaults. */
    cursor?: string;
    /** Provider option trim. See the operation reference for values and defaults. */
    trim?: boolean;
    /** Provider option usePost. See the operation reference for values and defaults. */
    usePost?: boolean;
}
/** Native reddit listComments fields; optional metadata may be missing or null. */
export interface RedditListCommentsData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider comments field; see the operation reference for its format. */
    "comments": ({
        "id"?: string | null;
        "author"?: string | null;
        "author_fullname"?: string | null;
        "body"?: string | null;
        "name"?: string | null;
        "created_utc"?: number | null;
        "created_at_iso"?: string | null;
        "parent_id"?: string | null;
        "url"?: string | null;
        "replies"?: {
            "items"?: ({
                "url"?: string | null;
                "created_utc"?: number | null;
                "created_at_iso"?: string | null;
                "subreddit_id"?: string | null;
                "approved_at_utc"?: unknown;
                "author_is_blocked"?: boolean | null;
                "comment_type"?: string | null;
                "awarders"?: unknown[] | null;
                "mod_reason_by"?: unknown;
                "banned_by"?: unknown;
                "author_flair_type"?: string | null;
                "total_awards_received"?: number | null;
                "subreddit"?: string | null;
                "author_flair_template_id"?: unknown;
                "likes"?: boolean | null;
                "replies"?: unknown;
                "user_reports"?: unknown[] | null;
                "saved"?: boolean | null;
                "id"?: string | null;
                "banned_at_utc"?: unknown;
                "mod_reason_title"?: unknown;
                "gilded"?: number | null;
                "archived"?: boolean | null;
                "collapsed_reason_code"?: unknown;
                "no_follow"?: boolean | null;
                "author"?: string | null;
                "can_mod_post"?: boolean | null;
                "send_replies"?: boolean | null;
                "parent_id"?: string | null;
                "score"?: number | null;
                "author_fullname"?: string | null;
                "removal_reason"?: unknown;
                "approved_by"?: unknown;
                "mod_note"?: unknown;
                "all_awardings"?: unknown[] | null;
                "body"?: string | null;
                "edited"?: boolean | number | null;
                "top_awarded_type"?: unknown;
                "author_flair_css_class"?: unknown;
                "name"?: string | null;
                "is_submitter"?: boolean | null;
                "downs"?: number | null;
                "author_flair_richtext"?: unknown[] | null;
                "author_patreon_flair"?: boolean | null;
                "body_html"?: string | null;
                "gildings"?: unknown;
                "collapsed_reason"?: unknown;
                "distinguished"?: unknown;
                "associated_award"?: unknown;
                "stickied"?: boolean | null;
                "author_premium"?: boolean | null;
                "can_gild"?: boolean | null;
                "link_id"?: string | null;
                "unrepliable_reason"?: unknown;
                "author_flair_text_color"?: unknown;
                "score_hidden"?: boolean | null;
                "permalink"?: string | null;
                "subreddit_type"?: string | null;
                "locked"?: boolean | null;
                "report_reasons"?: unknown;
                "created"?: number | null;
                "author_flair_text"?: unknown;
                "treatment_tags"?: unknown[] | null;
                "collapsed"?: boolean | null;
                "subreddit_name_prefixed"?: string | null;
                "controversiality"?: number | null;
                "depth"?: number | null;
                "author_flair_background_color"?: unknown;
                "collapsed_because_crowd_control"?: unknown;
                "mod_reports"?: unknown[] | null;
                "num_reports"?: number | null;
                "ups"?: number | null;
            })[] | null;
            "more"?: {
                "has_more"?: boolean | null;
                "cursor"?: string | null;
            } | null;
        } | null;
        "ups"?: number | null;
        "downs"?: number | null;
        "score"?: number | null;
    })[];
    /** Native provider post field; see the operation reference for its format. */
    "post": {
        "id"?: string | null;
        "author"?: string | null;
        "author_fullname"?: string | null;
        "subreddit"?: string | null;
        "title"?: string | null;
        "downs"?: number | null;
        "name"?: string | null;
        "upvote_ratio"?: number | null;
        "ups"?: number | null;
        "total_awards_received"?: number | null;
        "score"?: number | null;
        "created"?: number | null;
        "num_comments"?: number | null;
        "url"?: string | null;
        "subreddit_subscribers"?: number | null;
        "is_video"?: boolean | null;
        "created_utc"?: number | null;
    } | null;
    /** Native provider more field; see the operation reference for its format. */
    "more"?: {
        "has_more"?: boolean | null;
        "cursor"?: string | null;
    } | null;
}
/** Options for reddit.getTranscript. */
export interface RedditGetTranscriptOptions {
    /** Provider option url. See the operation reference for values and defaults. */
    url: string;
    /** Provider option language. See the operation reference for values and defaults. */
    language?: string;
    /** Provider option cacheMaxAge. See the operation reference for values and defaults. */
    cacheMaxAge?: "1d" | "3d" | "7d" | "14d" | "30d";
}
/** Native reddit getTranscript fields; optional metadata may be missing or null. */
export interface RedditGetTranscriptData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider url field; see the operation reference for its format. */
    "url"?: string | null;
    /** Native provider post_id field; see the operation reference for its format. */
    "post_id"?: string | null;
    /** Native provider video_id field; see the operation reference for its format. */
    "video_id"?: string | null;
    /** Native provider caption_url field; see the operation reference for its format. */
    "caption_url"?: string | null;
    /** Native provider language field; see the operation reference for its format. */
    "language"?: string | null;
    /** Native provider raw_vtt field; see the operation reference for its format. */
    "raw_vtt"?: string | null;
    /** Native transcript text or timestamped segments; null indicates unavailable captions. */
    "transcript": string | null;
    /** Native provider transcriptNotAvailable field; see the operation reference for its format. */
    "transcriptNotAvailable"?: boolean | null;
}
/** Options for reddit.search. */
export interface RedditSearchOptions {
    /** Provider option query. See the operation reference for values and defaults. */
    query: string;
    /** Provider option filter. See the operation reference for values and defaults. */
    filter?: "posts" | "comments";
    /** Provider option sort. See the operation reference for values and defaults. */
    sort?: "relevance" | "new" | "top" | "comment_count";
    /** Provider option timeframe. See the operation reference for values and defaults. */
    timeframe?: "all" | "day" | "week" | "month" | "year";
    /** Provider option after. See the operation reference for values and defaults. */
    after?: string;
    /** Provider option trim. See the operation reference for values and defaults. */
    trim?: boolean;
}
/** Native reddit search fields; optional metadata may be missing or null. */
export interface RedditSearchData {
    /** Provider success flag. Failures throw before a client returns. */
    success: boolean;
    /** Remaining provider credits, when reported. */
    credits_remaining?: number | null;
    /** Credits charged for this request, when reported. */
    credits_charged?: number | null;
    /** Whether the provider returned cached data. */
    cached?: boolean | null;
    /** Provider cache timestamp, preserved without conversion. */
    cached_at?: string | null;
    /** Provider partial-failure details; also retained in raw. */
    errors?: unknown;
    /** Provider warnings, also retained in raw. */
    warnings?: unknown;
    /** Native provider posts field; see the operation reference for its format. */
    "posts"?: ({
        "id"?: string | null;
        "author"?: string | null;
        "author_fullname"?: string | null;
        "subreddit"?: string | null;
        "title"?: string | null;
        "downs"?: number | null;
        "name"?: string | null;
        "upvote_ratio"?: number | null;
        "ups"?: number | null;
        "total_awards_received"?: number | null;
        "score"?: number | null;
        "created"?: number | null;
        "num_comments"?: number | null;
        "url"?: string | null;
        "subreddit_subscribers"?: number | null;
        "is_video"?: boolean | null;
        "created_utc"?: number | null;
    })[] | null;
    /** Opaque Reddit next-page token; pass back unchanged. */
    "after"?: string | null;
    /** Native provider comments field; see the operation reference for its format. */
    "comments"?: ({
        "id"?: string | null;
        "post_id"?: string | null;
        "parent_comment_id"?: string | null;
        "is_reply_to_comment"?: boolean | null;
        "author"?: string | null;
        "author_avatar"?: string | null;
        "author_nsfw"?: boolean | null;
        "body"?: string | null;
        "body_html"?: string | null;
        "images"?: unknown[] | null;
        "votes"?: number | null;
        "url"?: string | null;
        "permalink"?: string | null;
        "created_at"?: string | null;
        "created_at_iso"?: string | null;
        "post"?: {
            "id"?: string | null;
            "title"?: string | null;
            "url"?: string | null;
            "permalink"?: string | null;
            "nsfw"?: boolean | null;
            "spoiler"?: boolean | null;
            "votes"?: number | null;
            "num_comments"?: number | null;
            "created_at"?: string | null;
            "created_at_iso"?: string | null;
        } | null;
        "subreddit"?: {
            "id"?: string | null;
            "name"?: string | null;
            "nsfw"?: boolean | null;
            "quarantined"?: boolean | null;
            "icon"?: string | null;
        } | null;
        "position"?: number | null;
    })[] | null;
}
export { ScrapeCreatorsError, decodeResponse } from "./transport";
import { check } from "submilli:security";
import { RedditTransport } from "./reddit";
const redditTransport = new RedditTransport();
/** Reddit public-data operations through ScrapeCreators. */
export class RedditClient {
    /** getSubreddit through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/reddit.getSubreddit {}
     */
    getSubreddit(options: RedditGetSubredditOptions = {}): Result<RedditGetSubredditData> {
        check("scrapecreators.com/reddit.getSubreddit", {});
        return redditTransport.getSubreddit(options);
    }
    /** listSubredditPosts through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/reddit.listSubredditPosts {}
     */
    listSubredditPosts(options: RedditListSubredditPostsOptions): Result<RedditListSubredditPostsData> {
        check("scrapecreators.com/reddit.listSubredditPosts", {});
        return redditTransport.listSubredditPosts(options);
    }
    /** searchSubreddit through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/reddit.searchSubreddit {}
     */
    searchSubreddit(options: RedditSearchSubredditOptions): Result<RedditSearchSubredditData> {
        check("scrapecreators.com/reddit.searchSubreddit", {});
        return redditTransport.searchSubreddit(options);
    }
    /** getPost through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/reddit.getPost {}
     */
    getPost(options: RedditGetPostOptions): Result<RedditGetPostData> {
        check("scrapecreators.com/reddit.getPost", {});
        return redditTransport.getPost(options);
    }
    /** listComments through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/reddit.listComments {}
     */
    listComments(options: RedditListCommentsOptions): Result<RedditListCommentsData> {
        check("scrapecreators.com/reddit.listComments", {});
        return redditTransport.listComments(options);
    }
    /** getTranscript through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/reddit.getTranscript {}
     */
    getTranscript(options: RedditGetTranscriptOptions): Result<RedditGetTranscriptData> {
        check("scrapecreators.com/reddit.getTranscript", {});
        return redditTransport.getTranscript(options);
    }
    /** search through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/reddit.search {}
     */
    search(options: RedditSearchOptions): Result<RedditSearchData> {
        check("scrapecreators.com/reddit.search", {});
        return redditTransport.search(options);
    }
}
/** Reddit operations using the blueprint-bound ScrapeCreators key. */
export const reddit = new RedditClient();
import { YouTubeTransport } from "./youtube";
const youtubeTransport = new YouTubeTransport();
/** YouTube public-data operations through ScrapeCreators. */
export class YouTubeClient {
    /** getChannel through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.getChannel {}
     */
    getChannel(options: YouTubeGetChannelOptions = {}): Result<YouTubeGetChannelData> {
        check("scrapecreators.com/youtube.getChannel", {});
        return youtubeTransport.getChannel(options);
    }
    /** listChannelVideos through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.listChannelVideos {}
     */
    listChannelVideos(options: YouTubeListChannelVideosOptions = {}): Result<YouTubeListChannelVideosData> {
        check("scrapecreators.com/youtube.listChannelVideos", {});
        return youtubeTransport.listChannelVideos(options);
    }
    /** listChannelPlaylists through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.listChannelPlaylists {}
     */
    listChannelPlaylists(options: YouTubeListChannelPlaylistsOptions = {}): Result<YouTubeListChannelPlaylistsData> {
        check("scrapecreators.com/youtube.listChannelPlaylists", {});
        return youtubeTransport.listChannelPlaylists(options);
    }
    /** listChannelLives through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.listChannelLives {}
     */
    listChannelLives(options: YouTubeListChannelLivesOptions = {}): Result<YouTubeListChannelLivesData> {
        check("scrapecreators.com/youtube.listChannelLives", {});
        return youtubeTransport.listChannelLives(options);
    }
    /** listChannelCommunityPosts through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.listChannelCommunityPosts {}
     */
    listChannelCommunityPosts(options: YouTubeListChannelCommunityPostsOptions = {}): Result<YouTubeListChannelCommunityPostsData> {
        check("scrapecreators.com/youtube.listChannelCommunityPosts", {});
        return youtubeTransport.listChannelCommunityPosts(options);
    }
    /** listChannelShorts through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.listChannelShorts {}
     */
    listChannelShorts(options: YouTubeListChannelShortsOptions = {}): Result<YouTubeListChannelShortsData> {
        check("scrapecreators.com/youtube.listChannelShorts", {});
        return youtubeTransport.listChannelShorts(options);
    }
    /** getVideo through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.getVideo {}
     */
    getVideo(options: YouTubeGetVideoOptions): Result<YouTubeGetVideoData> {
        check("scrapecreators.com/youtube.getVideo", {});
        return youtubeTransport.getVideo(options);
    }
    /** getTranscript through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.getTranscript {}
     */
    getTranscript(options: YouTubeGetTranscriptOptions): Result<YouTubeGetTranscriptData> {
        check("scrapecreators.com/youtube.getTranscript", {});
        return youtubeTransport.getTranscript(options);
    }
    /** getSponsors through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.getSponsors {}
     */
    getSponsors(options: YouTubeGetSponsorsOptions): Result<YouTubeGetSponsorsData> {
        check("scrapecreators.com/youtube.getSponsors", {});
        return youtubeTransport.getSponsors(options);
    }
    /** search through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.search {}
     */
    search(options: YouTubeSearchOptions): Result<YouTubeSearchData> {
        check("scrapecreators.com/youtube.search", {});
        return youtubeTransport.search(options);
    }
    /** searchTypeahead through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.searchTypeahead {}
     */
    searchTypeahead(options: YouTubeSearchTypeaheadOptions): Result<YouTubeSearchTypeaheadData> {
        check("scrapecreators.com/youtube.searchTypeahead", {});
        return youtubeTransport.searchTypeahead(options);
    }
    /** searchHashtag through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.searchHashtag {}
     */
    searchHashtag(options: YouTubeSearchHashtagOptions): Result<YouTubeSearchHashtagData> {
        check("scrapecreators.com/youtube.searchHashtag", {});
        return youtubeTransport.searchHashtag(options);
    }
    /** listComments through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.listComments {}
     */
    listComments(options: YouTubeListCommentsOptions): Result<YouTubeListCommentsData> {
        check("scrapecreators.com/youtube.listComments", {});
        return youtubeTransport.listComments(options);
    }
    /** listCommentReplies through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.listCommentReplies {}
     */
    listCommentReplies(options: YouTubeListCommentRepliesOptions): Result<YouTubeListCommentRepliesData> {
        check("scrapecreators.com/youtube.listCommentReplies", {});
        return youtubeTransport.listCommentReplies(options);
    }
    /** listTrendingShorts through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.listTrendingShorts {}
     */
    listTrendingShorts(options: YouTubeListTrendingShortsOptions = {}): Result<YouTubeListTrendingShortsData> {
        check("scrapecreators.com/youtube.listTrendingShorts", {});
        return youtubeTransport.listTrendingShorts(options);
    }
    /** getPlaylist through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.getPlaylist {}
     */
    getPlaylist(options: YouTubeGetPlaylistOptions): Result<YouTubeGetPlaylistData> {
        check("scrapecreators.com/youtube.getPlaylist", {});
        return youtubeTransport.getPlaylist(options);
    }
    /** getCommunityPost through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/youtube.getCommunityPost {}
     */
    getCommunityPost(options: YouTubeGetCommunityPostOptions): Result<YouTubeGetCommunityPostData> {
        check("scrapecreators.com/youtube.getCommunityPost", {});
        return youtubeTransport.getCommunityPost(options);
    }
}
/** YouTube operations using the blueprint-bound ScrapeCreators key. */
export const youtube = new YouTubeClient();
import { LinkedInTransport } from "./linkedin";
const linkedinTransport = new LinkedInTransport();
/** LinkedIn public-data operations through ScrapeCreators. */
export class LinkedInClient {
    /** getProfile through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/linkedin.getProfile {}
     */
    getProfile(options: LinkedInGetProfileOptions): Result<LinkedInGetProfileData> {
        check("scrapecreators.com/linkedin.getProfile", {});
        return linkedinTransport.getProfile(options);
    }
    /** getCompany through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/linkedin.getCompany {}
     */
    getCompany(options: LinkedInGetCompanyOptions): Result<LinkedInGetCompanyData> {
        check("scrapecreators.com/linkedin.getCompany", {});
        return linkedinTransport.getCompany(options);
    }
    /** listCompanyPosts through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/linkedin.listCompanyPosts {}
     */
    listCompanyPosts(options: LinkedInListCompanyPostsOptions): Result<LinkedInListCompanyPostsData> {
        check("scrapecreators.com/linkedin.listCompanyPosts", {});
        return linkedinTransport.listCompanyPosts(options);
    }
    /** searchPosts through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/linkedin.searchPosts {}
     */
    searchPosts(options: LinkedInSearchPostsOptions): Result<LinkedInSearchPostsData> {
        check("scrapecreators.com/linkedin.searchPosts", {});
        return linkedinTransport.searchPosts(options);
    }
    /** getPost through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/linkedin.getPost {}
     */
    getPost(options: LinkedInGetPostOptions): Result<LinkedInGetPostData> {
        check("scrapecreators.com/linkedin.getPost", {});
        return linkedinTransport.getPost(options);
    }
    /** getTranscript through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/linkedin.getTranscript {}
     */
    getTranscript(options: LinkedInGetTranscriptOptions): Result<LinkedInGetTranscriptData> {
        check("scrapecreators.com/linkedin.getTranscript", {});
        return linkedinTransport.getTranscript(options);
    }
}
/** LinkedIn operations using the blueprint-bound ScrapeCreators key. */
export const linkedin = new LinkedInClient();
import { GitHubTransport } from "./github";
const githubTransport = new GitHubTransport();
/** GitHub public-data operations through ScrapeCreators. */
export class GitHubClient {
    /** getUser through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.getUser {}
     */
    getUser(options: GitHubGetUserOptions = {}): Result<GitHubGetUserData> {
        check("scrapecreators.com/github.getUser", {});
        return githubTransport.getUser(options);
    }
    /** listUserRepositories through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.listUserRepositories {}
     */
    listUserRepositories(options: GitHubListUserRepositoriesOptions = {}): Result<GitHubListUserRepositoriesData> {
        check("scrapecreators.com/github.listUserRepositories", {});
        return githubTransport.listUserRepositories(options);
    }
    /** listUserPullRequests through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.listUserPullRequests {}
     */
    listUserPullRequests(options: GitHubListUserPullRequestsOptions): Result<GitHubListUserPullRequestsData> {
        check("scrapecreators.com/github.listUserPullRequests", {});
        return githubTransport.listUserPullRequests(options);
    }
    /** listUserActivity through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.listUserActivity {}
     */
    listUserActivity(options: GitHubListUserActivityOptions = {}): Result<GitHubListUserActivityData> {
        check("scrapecreators.com/github.listUserActivity", {});
        return githubTransport.listUserActivity(options);
    }
    /** listFollowers through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.listFollowers {}
     */
    listFollowers(options: GitHubListFollowersOptions = {}): Result<GitHubListFollowersData> {
        check("scrapecreators.com/github.listFollowers", {});
        return githubTransport.listFollowers(options);
    }
    /** listFollowing through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.listFollowing {}
     */
    listFollowing(options: GitHubListFollowingOptions = {}): Result<GitHubListFollowingData> {
        check("scrapecreators.com/github.listFollowing", {});
        return githubTransport.listFollowing(options);
    }
    /** getContributions through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.getContributions {}
     */
    getContributions(options: GitHubGetContributionsOptions = {}): Result<GitHubGetContributionsData> {
        check("scrapecreators.com/github.getContributions", {});
        return githubTransport.getContributions(options);
    }
    /** getRepository through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.getRepository {}
     */
    getRepository(options: GitHubGetRepositoryOptions): Result<GitHubGetRepositoryData> {
        check("scrapecreators.com/github.getRepository", {});
        return githubTransport.getRepository(options);
    }
    /** listTrendingRepositories through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.listTrendingRepositories {}
     */
    listTrendingRepositories(options: GitHubListTrendingRepositoriesOptions = {}): Result<GitHubListTrendingRepositoriesData> {
        check("scrapecreators.com/github.listTrendingRepositories", {});
        return githubTransport.listTrendingRepositories(options);
    }
    /** listTrendingDevelopers through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/github.listTrendingDevelopers {}
     */
    listTrendingDevelopers(options: GitHubListTrendingDevelopersOptions = {}): Result<GitHubListTrendingDevelopersData> {
        check("scrapecreators.com/github.listTrendingDevelopers", {});
        return githubTransport.listTrendingDevelopers(options);
    }
}
/** GitHub operations using the blueprint-bound ScrapeCreators key. */
export const github = new GitHubClient();
import { TwitterTransport } from "./twitter";
const twitterTransport = new TwitterTransport();
/** Twitter public-data operations through ScrapeCreators. */
export class TwitterClient {
    /** searchPosts through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/twitter.searchPosts {}
     */
    searchPosts(options: TwitterSearchPostsOptions): Result<TwitterSearchPostsData> {
        check("scrapecreators.com/twitter.searchPosts", {});
        return twitterTransport.searchPosts(options);
    }
    /** getProfile through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/twitter.getProfile {}
     */
    getProfile(options: TwitterGetProfileOptions): Result<TwitterGetProfileData> {
        check("scrapecreators.com/twitter.getProfile", {});
        return twitterTransport.getProfile(options);
    }
    /** listUserTweets through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/twitter.listUserTweets {}
     */
    listUserTweets(options: TwitterListUserTweetsOptions): Result<TwitterListUserTweetsData> {
        check("scrapecreators.com/twitter.listUserTweets", {});
        return twitterTransport.listUserTweets(options);
    }
    /** getTweet through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/twitter.getTweet {}
     */
    getTweet(options: TwitterGetTweetOptions): Result<TwitterGetTweetData> {
        check("scrapecreators.com/twitter.getTweet", {});
        return twitterTransport.getTweet(options);
    }
    /** getTranscript through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/twitter.getTranscript {}
     */
    getTranscript(options: TwitterGetTranscriptOptions): Result<TwitterGetTranscriptData> {
        check("scrapecreators.com/twitter.getTranscript", {});
        return twitterTransport.getTranscript(options);
    }
    /** getCommunity through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/twitter.getCommunity {}
     */
    getCommunity(options: TwitterGetCommunityOptions): Result<TwitterGetCommunityData> {
        check("scrapecreators.com/twitter.getCommunity", {});
        return twitterTransport.getCommunity(options);
    }
    /** listCommunityTweets through ScrapeCreators.
     * @param options Provider controls; omit optional values for provider defaults.
     * @returns One typed provider response plus its full raw JSON.
     * @capability scrapecreators.com/twitter.listCommunityTweets {}
     */
    listCommunityTweets(options: TwitterListCommunityTweetsOptions): Result<TwitterListCommunityTweetsData> {
        check("scrapecreators.com/twitter.listCommunityTweets", {});
        return twitterTransport.listCommunityTweets(options);
    }
}
/** Twitter operations using the blueprint-bound ScrapeCreators key. */
export const twitter = new TwitterClient();
/** All platform clients, available through a named aggregate import. */
export const scrapecreators = { reddit, youtube, linkedin, github, twitter };
